//! `api-parity` (0.5.2): a framework's API as data, so an invented call
//! is caught before the compiler (or a reviewer) has to.
//!
//! An installed plugin may carry `api.toml` — the public surface of the
//! crate it mirrors, generated from that release's source. A rule binding
//! this check (the gpui-kit plugin's `never-invent-an-api`) makes every
//! path into that crate (`gpui_kit::component::button::Button`, `use`
//! groups included) resolve against it: an unknown module or item is
//! named, with the version it was checked against. Open modules (glob
//! re-exports) and names outside the listed modules are trusted — the
//! check reports only what it can be sure of.

use std::collections::HashMap;

use regex::Regex;

use super::Violation;
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::path_within;
use crate::schema::store::plugins::Api;
use crate::search::digest::live::live_code;

/// The surfaces in force for this run: (rule id, rationale) per api.
pub struct Surfaces<'a> {
    apis: Vec<(
        &'a Api,
        HashMap<&'a str, &'a crate::schema::store::plugins::ApiModule>,
        Regex,
    )>,
    rule: Option<(String, String, Vec<String>)>,
}

pub fn prepare<'a>(rules: &Rules, env: Option<&str>, apis: &'a [Api]) -> Surfaces<'a> {
    let rule = rules
        .rules
        .iter()
        .find(|r| {
            !r.is_deprecated() && r.matches_env(env) && r.check.as_deref() == Some("api-parity")
        })
        .map(|r| (r.id.clone(), r.rationale.clone(), r.scope.clone()));
    let apis = apis
        .iter()
        .filter_map(|a| {
            let re = Regex::new(&format!(
                r"\b{}::((?:[A-Za-z_][A-Za-z0-9_]*::)*)(\{{[^}}]*\}}|[A-Za-z_][A-Za-z0-9_]*)",
                regex::escape(&a.crate_name)
            ))
            .ok()?;
            let by_path = a.modules.iter().map(|m| (m.path.as_str(), m)).collect();
            Some((a, by_path, re))
        })
        .collect();
    Surfaces { apis, rule }
}

pub fn check(s: &Surfaces, file: &str, src: &str, out: &mut Vec<Violation>) {
    let Some((rule_id, rationale, scope)) = &s.rule else {
        return;
    };
    if !scope.is_empty() && !scope.iter().any(|p| path_within(file, p)) {
        return;
    }
    for (api, modules, re) in &s.apis {
        if !src.contains(api.crate_name.as_str()) {
            continue;
        }
        let live = live_code(src);
        let mut report = |n: usize, module: &str, name: &str| {
            if let Some(problem) = resolve(modules, module, name) {
                out.push(Violation {
                    check: "api-parity".into(),
                    file: file.into(),
                    line: n,
                    severity: crate::enforcement::checks::severity_of("api-parity"),
                    message: format!(
                        "{problem} in {} {} (rule `{rule_id}`)",
                        api.crate_name, api.version
                    ),
                    hint: format!(
                        "search the real signature (docs.rs / the crate source) — {rationale}"
                    ),
                });
            }
        };
        // `use` statements read whole (0.5.9): joined across lines, nested
        // groups carrying their path prefix into each name
        let mut i = 0;
        while i < live.len() {
            let (n, line) = live[i];
            let body = line
                .strip_prefix("pub use ")
                .or_else(|| line.strip_prefix("use "))
                .or_else(|| line.strip_prefix("pub(crate) use "));
            let Some(first) = body else {
                for cap in re.captures_iter(line) {
                    if !cap[2].starts_with('{') {
                        report(n, cap[1].trim_end_matches("::"), &cap[2]);
                    }
                }
                i += 1;
                continue;
            };
            let mut stmt = first.to_string();
            while !stmt.contains(';') && i + 1 < live.len() {
                i += 1;
                stmt.push_str(live[i].1);
            }
            i += 1;
            let stmt = stmt.trim().trim_end_matches(';').trim();
            let crate_prefix = format!("{}::", api.crate_name);
            let Some(rest) = stmt.strip_prefix(&crate_prefix) else {
                continue;
            };
            let mut paths = Vec::new();
            expand("", rest, &mut paths);
            for path in paths {
                let (module, name) = match path.rsplit_once("::") {
                    Some((m, nm)) => (m.to_string(), nm.to_string()),
                    None => (String::new(), path.clone()),
                };
                report(n, &module, &name);
            }
        }
    }
}

/// The full paths a `use` tree names: `a::{B, c::{D, E as F}, self}`
/// under `prefix` → `a::B`, `a::c::D`, `a::c::E`, `a`; globs are skipped.
fn expand(prefix: &str, tree: &str, out: &mut Vec<String>) {
    let join = |p: &str, x: &str| {
        if p.is_empty() {
            x.to_string()
        } else if x.is_empty() {
            p.to_string()
        } else {
            format!("{p}::{x}")
        }
    };
    let tree = tree.trim();
    if let Some(open) = tree.find('{') {
        let head = tree[..open].trim().trim_end_matches("::");
        let inner = tree[open + 1..].trim_end().trim_end_matches('}');
        let base = join(prefix, head);
        let mut depth = 0;
        let mut start = 0;
        let bytes: Vec<char> = inner.chars().collect();
        let mut parts = Vec::new();
        for (k, c) in bytes.iter().enumerate() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(bytes[start..k].iter().collect::<String>());
                    start = k + 1;
                }
                _ => {}
            }
        }
        parts.push(bytes[start..].iter().collect::<String>());
        for part in parts {
            let part = part.trim();
            if part.is_empty() || part == "*" {
                continue;
            }
            if part == "self" {
                out.push(base.clone());
            } else {
                expand(&base, part, out);
            }
        }
    } else {
        let item = tree.split(" as ").next().unwrap_or("").trim();
        if !item.is_empty() && !item.ends_with('*') {
            out.push(join(prefix, item));
        }
    }
}

/// Why `module::name` is not in the surface, or None when it is (or the
/// surface cannot tell).
fn resolve(
    modules: &HashMap<&str, &crate::schema::store::plugins::ApiModule>,
    module: &str,
    name: &str,
) -> Option<String> {
    let full = if module.is_empty() {
        name.to_string()
    } else {
        format!("{module}::{name}")
    };
    if modules.contains_key(full.as_str()) {
        return None; // a module named in full
    }
    match modules.get(module) {
        Some(m) if m.open || m.items.iter().any(|i| i == name) => None,
        Some(_) => Some(format!(
            "`{name}` is not in `{}`",
            if module.is_empty() { "<root>" } else { module }
        )),
        None => {
            // the nearest listed ancestor decides: open trusts, closed knows
            let mut segs: Vec<&str> = module.split("::").collect();
            while !segs.is_empty() {
                segs.pop();
                let up = segs.join("::");
                if let Some(m) = modules.get(up.as_str()) {
                    if m.open {
                        return None;
                    }
                    let first = module[up.len()..]
                        .trim_start_matches("::")
                        .split("::")
                        .next()?;
                    return (!m.items.iter().any(|i| i == first))
                        .then(|| format!("module `{module}` does not exist"));
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::rules::Rule;
    use crate::schema::store::plugins::ApiModule;

    fn api() -> Api {
        let m = |path: &str, items: &[&str], open: bool| ApiModule {
            path: path.into(),
            items: items.iter().map(|s| s.to_string()).collect(),
            open,
        };
        Api {
            crate_name: "gpui_kit".into(),
            version: "0.8.0".into(),
            modules: vec![
                m("", &["component", "init", "open_window"], true),
                m("component", &["button", "input"], false),
                m("component::button", &["Button", "ButtonGroup"], false),
                m("component::input", &["Input", "InputState"], false),
            ],
        }
    }

    /// 0.5.2: real paths pass, an invented item or module is named with
    /// the version; the root (gpui's glob) trusts everything; `use` groups
    /// are read name by name; a comment never counts.
    #[test]
    fn paths_into_the_crate_resolve_against_its_surface() {
        let mut rules = Rules::default();
        rules.rules.push(Rule {
            id: "never-invent-an-api".into(),
            check: Some("api-parity".into()),
            ..Default::default()
        });
        let apis = vec![api()];
        let s = prepare(&rules, None, &apis);
        let src = "use gpui_kit::*;\nuse gpui_kit::component::button::{Button, Buttn as B};\nuse gpui_kit::component::input::InputState;\nuse gpui_kit::component::textfield::TextField;\nfn f() { gpui_kit::init(cx); gpui_kit::div(); }\n// gpui_kit::component::nope::X\n";
        let mut out = Vec::new();
        check(&s, "src/view.rs", src, &mut out);
        let msgs: Vec<(usize, &str)> = out.iter().map(|v| (v.line, v.message.as_str())).collect();
        assert_eq!(out.len(), 2, "{msgs:?}");
        assert!(
            out[0]
                .message
                .contains("`Buttn` is not in `component::button`"),
            "{msgs:?}"
        );
        assert!(out[0].message.contains("gpui_kit 0.8.0"));
        assert!(
            out[1]
                .message
                .contains("module `component::textfield` does not exist"),
            "{msgs:?}"
        );
        let unbound = prepare(&Rules::default(), None, &apis);
        let mut out = Vec::new();
        check(&unbound, "src/view.rs", src, &mut out);
        assert!(out.is_empty(), "no rule bound: nothing runs");
    }

    /// 0.5.9 review M7: a `use` group spanning lines is read, and a nested
    /// group carries its prefix (no false "not in component::input").
    #[test]
    fn use_trees_are_read_whole() {
        let mut rules = Rules::default();
        rules.rules.push(Rule {
            id: "never-invent-an-api".into(),
            check: Some("api-parity".into()),
            ..Default::default()
        });
        let mut a = api();
        a.modules.push(ApiModule {
            path: "component::input::popovers".into(),
            items: vec!["editor_popover".into()],
            open: false,
        });
        a.modules[3].items.push("popovers".into());
        let apis = vec![a];
        let s = prepare(&rules, None, &apis);
        let src = concat!(
            "use gpui_kit::component::button::{\n",
            "    Button,\n",
            "    Buttn2,\n",
            "};\n",
            "use gpui_kit::component::input::{InputState, popovers::{editor_popover, self}};\n",
            "use gpui_kit::component::{button, input::Input};\n",
        );
        let mut out = Vec::new();
        check(&s, "src/v.rs", src, &mut out);
        let msgs: Vec<&str> = out.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(out.len(), 1, "{msgs:?}");
        assert!(
            msgs[0].contains("`Buttn2` is not in `component::button`"),
            "{msgs:?}"
        );
    }
}
