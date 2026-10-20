//! `registry` (0.5.3): what a declaration registers must be used, and what
//! a binding registers must not collide — a generic engine plugins bind
//! as data (gpui-kit: `actions!` declarations and `KeyBinding`s).
//!
//! The rule's `values`, each a regex:
//! - `declare:<re>` — group 1 holds the declared names (comma-separated:
//!   `actions!(app, [Quit, Copy])` → `Quit`, `Copy`);
//! - `use:<re>` — what counts as using a name, `NAME` standing for it
//!   (default: the name, word-bounded, on any other live line);
//! - `bind:<re>` — group 1 the key, group 2 (optional) its scope: the same
//!   key bound twice in one scope is a collision.
//!
//! Project-wide (every live source line at once), text only — the
//! rationale says what the names stand for.

use std::collections::BTreeMap;

use regex::Regex;

use crate::enforcement::violation::{Severity, Violation};
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::path_within;
use crate::search::digest::live::live_code;

pub fn check(
    rules: &Rules,
    env: Option<&str>,
    srcs: &[(String, String)],
    out: &mut Vec<Violation>,
) {
    for rule in rules.rules.iter().filter(|r| {
        !r.is_deprecated() && r.matches_env(env) && r.check.as_deref() == Some("registry")
    }) {
        let get = |k: &str| {
            rule.values
                .iter()
                .find_map(|v| v.strip_prefix(&format!("{k}:")).map(str::to_string))
        };
        let compile = |s: Option<String>| s.map(|s| Regex::new(&s).map_err(|e| (s, e)));
        let (declare, use_tpl, bind) = (get("declare"), get("use"), compile(get("bind")));
        let declare = compile(declare);
        let bad = [&declare, &bind]
            .iter()
            .filter_map(|r| r.as_ref().and_then(|r| r.as_ref().err()))
            .map(|(s, e)| format!("`{s}`: {e}"))
            .next();
        if bad.is_some() || (declare.is_none() && bind.is_none()) {
            out.push(Violation {
                check: "harna-data".into(),
                file: ".harna/rules.toml".into(),
                line: 0,
                severity: Severity::Error,
                message: format!(
                    "rule `{}`: {} — it checks nothing until fixed",
                    rule.id,
                    bad.unwrap_or_else(
                        || "registry needs declare:<regex> or bind:<regex> values".into()
                    )
                ),
                hint: "fix the rule's values (harna rules shows it)".into(),
            });
            continue;
        }
        let files: Vec<(&str, Vec<(usize, &str)>)> = srcs
            .iter()
            .filter(|(f, _)| rule.scope.is_empty() || rule.scope.iter().any(|s| path_within(f, s)))
            .map(|(f, s)| (f.as_str(), live_code(s)))
            .collect();
        let severity = crate::enforcement::checks::severity_of("registry");
        if let Some(Ok(declare)) = declare {
            // (name, file, line) of every declaration
            let mut declared = Vec::new();
            for (f, lines) in &files {
                for (n, line) in lines {
                    for cap in declare.captures_iter(line) {
                        let Some(group) = cap.get(1) else { continue };
                        for name in group.as_str().split(',') {
                            let name = name.trim().trim_start_matches("#[").trim();
                            if !name.is_empty()
                                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                            {
                                declared.push((name.to_string(), *f, *n));
                            }
                        }
                    }
                }
            }
            for (name, f, n) in declared {
                let tpl = use_tpl.clone().unwrap_or_else(|| r"\bNAME\b".into());
                let Ok(used) = Regex::new(&tpl.replace("NAME", &regex::escape(&name))) else {
                    continue;
                };
                let is_used = files.iter().any(|(g, lines)| {
                    lines
                        .iter()
                        .any(|(m, l)| !(*g == f && *m == n) && used.is_match(l))
                });
                if !is_used {
                    out.push(Violation {
                        check: "registry".into(),
                        file: f.into(),
                        line: n,
                        severity,
                        message: format!(
                            "`{name}` is declared but never used (rule `{}`)",
                            rule.id
                        ),
                        hint: format!("handle or remove it — {}", rule.rationale),
                    });
                }
            }
        }
        if let Some(Ok(bind)) = bind {
            let mut seen: BTreeMap<(String, String), (String, usize)> = BTreeMap::new();
            for (f, lines) in &files {
                for (n, line) in lines {
                    for cap in bind.captures_iter(line) {
                        let key = cap
                            .get(1)
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default();
                        let scope = cap
                            .get(2)
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default();
                        if let Some((pf, pn)) = seen.get(&(key.clone(), scope.clone())) {
                            out.push(Violation {
                                check: "registry".into(),
                                file: (*f).into(),
                                line: *n,
                                severity,
                                message: format!(
                                    "`{key}`{} is bound again — first at {pf}:{pn} (rule `{}`)",
                                    if scope.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" in `{scope}`")
                                    },
                                    rule.id
                                ),
                                hint: format!(
                                    "one key, one meaning per scope — {}",
                                    rule.rationale
                                ),
                            });
                        } else {
                            seen.insert((key, scope), ((*f).to_string(), *n));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::rules::Rule;

    /// 0.5.3: a declared action nobody handles is named; one handled
    /// elsewhere is not; a key bound twice in one context collides, the
    /// same key in another context does not.
    #[test]
    fn declarations_are_used_and_bindings_unique() {
        let mut rules = Rules::default();
        rules.rules.push(Rule {
            id: "actions-handled".into(),
            check: Some("registry".into()),
            values: vec![
                r"declare:actions!\(\s*\w+\s*,\s*\[([^\]]*)\]".into(),
                r#"bind:KeyBinding::new\(\s*"([^"]+)"\s*,\s*[\w:]+\s*,\s*(?:Some\(\s*"([^"]+)"\s*\)|None)"#.into(),
            ],
            ..Default::default()
        });
        let srcs = vec![
            (
                "src/actions.rs".to_string(),
                "actions!(app, [Quit, Copy, Orphan]);\n".to_string(),
            ),
            (
                "src/view.rs".to_string(),
                "fn quit(&mut self, _: &Quit) {}\nfn copy(_: &Copy) {}\nKeyBinding::new(\"cmd-q\", Quit, None);\nKeyBinding::new(\"cmd-c\", Copy, Some(\"Editor\"));\nKeyBinding::new(\"cmd-c\", Copy, Some(\"List\"));\nKeyBinding::new(\"cmd-q\", Copy, None);\n".to_string(),
            ),
        ];
        let mut out = Vec::new();
        check(&rules, None, &srcs, &mut out);
        let msgs: Vec<&str> = out.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(out.len(), 2, "{msgs:?}");
        assert!(
            msgs[0].contains("`Orphan` is declared but never used"),
            "{msgs:?}"
        );
        assert!(
            msgs[1].contains("`cmd-q` is bound again — first at src/view.rs:3"),
            "{msgs:?}"
        );
        let mut bad = Rules::default();
        bad.rules.push(Rule {
            id: "x".into(),
            check: Some("registry".into()),
            values: vec!["declare:(".into()],
            ..Default::default()
        });
        let mut out = Vec::new();
        check(&bad, None, &srcs, &mut out);
        assert_eq!(out[0].check, "harna-data");
    }

    /// The shipped gpui-kit rules read real gpui-kit spellings.
    #[test]
    fn the_gpui_kit_pack_binds_actions_and_keys() {
        let pack = crate::generation::structure::plugin::resolve("gpui-kit", None).unwrap();
        let mut rules = pack.rules();
        rules
            .rules
            .retain(|r| r.check.as_deref() == Some("registry"));
        assert_eq!(rules.rules.len(), 2);
        let srcs = vec![(
            "src/app.rs".to_string(),
            concat!(
                "actions!(ledger, [NewEntry, Unused]);\n",
                "fn new_entry(&mut self, _: &NewEntry, w: &mut Window, cx: &mut Context<Self>) {}\n",
                "cx.bind_keys([KeyBinding::new(\"cmd-n\", NewEntry, Some(\"Ledger\")),\n",
                "    KeyBinding::new(\"cmd-n\", NewEntry, Some(\"Ledger\"))]);\n",
            )
            .to_string(),
        )];
        let mut out = Vec::new();
        check(&rules, None, &srcs, &mut out);
        let msgs: Vec<&str> = out.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(out.len(), 2, "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("`Unused` is declared")));
        assert!(msgs
            .iter()
            .any(|m| m.contains("`cmd-n` in `Ledger` is bound again")));
    }
}
