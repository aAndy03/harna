//! Component `contract-freeze` (0.7.2, 0.7.7): freeze a container's
//! surface as a versioned contract. The surface is its declared `provides`
//! (every public item when none is declared), each with its normalized
//! declaration. Re-freezing is how a change is accepted — and the version
//! must say what changed (0.7.7): a removal or re-signature needs the next
//! major (minor in 0.x), an addition the next minor.

use std::collections::BTreeMap;
use std::path::Path;

use crate::schema::model::manifest::Manifest;
use crate::schema::store::contracts::{parse_version, Contract};
use crate::search::digest::Digest;

/// The container's current surface: provided name -> signature.
pub fn surface_now(
    root: &Path,
    m: &Manifest,
    d: &Digest,
    container: &str,
) -> anyhow::Result<BTreeMap<String, String>> {
    let c = m
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .find(|c| c.name == container)
        .ok_or_else(|| anyhow::anyhow!("no container `{container}`"))?;
    if crate::schema::model::scope::normalize_scope(&c.path).is_empty() {
        anyhow::bail!("container `{container}` has no folder — a contract is frozen from its code");
    }
    let mut provided: Vec<String> = c.provides.clone();
    for comp in &c.components {
        provided.extend(comp.provides.iter().cloned());
    }
    let mut out = BTreeMap::new();
    for md in d
        .modules
        .iter()
        .filter(|md| crate::schema::model::scope::path_in_container(&md.path, &c.path))
    {
        let Ok(src) = std::fs::read_to_string(root.join(&md.path)) else {
            continue;
        };
        for (key, sig) in declarations(&src, md.lang == "rust") {
            let last = key.rsplit("::").next().unwrap_or(&key).to_string();
            if provided.is_empty() || provided.contains(&key) || provided.contains(&last) {
                out.entry(key).or_insert(sig);
            }
        }
    }
    Ok(out)
}

/// What changed between two surfaces: (removed or re-signed, added).
pub fn diff(
    was: &BTreeMap<String, String>,
    now: &BTreeMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let broken = was
        .iter()
        .filter(|(k, v)| now.get(*k) != Some(v))
        .map(|(k, _)| k.clone())
        .collect();
    let added = now
        .keys()
        .filter(|k| !was.contains_key(*k))
        .cloned()
        .collect();
    (broken, added)
}

/// A new contract for `container` at `version`. The version is strict
/// semver; against the previous contract it must say what changed.
pub fn freeze(
    root: &Path,
    m: &Manifest,
    d: &Digest,
    container: &str,
    version: &str,
    previous: Option<&Contract>,
    today: &str,
) -> anyhow::Result<Contract> {
    let v = parse_version(version).ok_or_else(|| {
        anyhow::anyhow!("`{version}` is not a version — use semver, e.g. 1.0.0 or 2.0.0-beta.1")
    })?;
    let items = surface_now(root, m, d, container)?;
    if items.is_empty() {
        anyhow::bail!(
            "container `{container}` exposes nothing yet (run `harna scan`, or declare provides)"
        );
    }
    if let Some(p) = previous {
        let pv = parse_version(&p.version).unwrap_or_default();
        if v < pv {
            anyhow::bail!(
                "contract `{container}` is at {}; {version} would go backwards",
                p.version
            );
        }
        let (broken, added) = diff(&p.items, &items);
        let next_major = if pv.0 == 0 {
            (0, pv.1 + 1, 0)
        } else {
            (pv.0 + 1, 0, 0)
        };
        let next_minor = (pv.0, pv.1 + 1, 0);
        let core = (v.0, v.1, v.2);
        if !broken.is_empty() && core < next_major {
            anyhow::bail!(
                "{} breaking change(s) ({}) need {}.{}.{} or later — a breaking change is never frozen at {version}",
                broken.len(),
                broken.join(", "),
                next_major.0,
                next_major.1,
                next_major.2
            );
        }
        if !added.is_empty() && core < next_minor {
            anyhow::bail!(
                "{} addition(s) ({}) need {}.{}.0 or later",
                added.len(),
                added.join(", "),
                next_minor.0,
                next_minor.1
            );
        }
    }
    Ok(Contract {
        container: container.into(),
        version: version.into(),
        frozen: today.into(),
        items,
    })
}

/// A declaration as a contract holds it (0.7.7): from its first line to
/// the body (`{`) or the end (`;`) across lines, a const's value cut off
/// (`= …`), whitespace normalized — so a rustfmt wrap, a body edit or a
/// space before `:` never reads as a change, and a parameter on line three
/// does.
fn normalize_decl(text: &str, cut_value: bool) -> String {
    let mut out = String::new();
    let mut depth: i32 = 0;
    for c in text.chars() {
        match c {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' => depth -= 1,
            '{' | ';' if depth <= 0 => break,
            '=' if cut_value && depth <= 0 => break,
            _ => {}
        }
        out.push(c);
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    // no space around punctuation: `(a: u8)` == `(a:u8)`
    let mut tight = String::with_capacity(collapsed.len());
    let chars: Vec<char> = collapsed.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c == ' ' {
            let before = i.checked_sub(1).map(|j| chars[j]).unwrap_or(' ');
            let after = chars.get(i + 1).copied().unwrap_or(' ');
            let punct = |x: char| "(),:<>&[];=*".contains(x);
            if punct(before) || punct(after) {
                continue;
            }
        }
        tight.push(*c);
    }
    // a trailing comma (rustfmt's wrapped lists) is no change either
    let tight = tight
        .replace(",)", ")")
        .replace(",]", "]")
        .replace(",>", ">");
    tight.trim_end_matches([' ', '-']).to_string()
}

/// Public declarations of one source file: (key, normalized signature).
/// Rust methods are keyed `Type::name` (two `new`s never collide).
pub fn declarations(src: &str, rust: bool) -> Vec<(String, String)> {
    static R: std::sync::OnceLock<(regex::Regex, regex::Regex, regex::Regex)> =
        std::sync::OnceLock::new();
    let (fn_re, item_re, impl_re) = R.get_or_init(|| {
        (
            regex::Regex::new(r"\bfn\s+([A-Za-z_]\w*)").expect("static"),
            regex::Regex::new(
                r"\b(struct|enum|trait|type|const|static|union|mod|function|class|interface|let|var)\s*\*?\s+([A-Za-z_$][\w$]*)",
            )
            .expect("static"),
            regex::Regex::new(r"^impl\b(?:<[^>]*>)?\s+(?:[\w:<>, ]+\s+for\s+)?([A-Za-z_]\w*)")
                .expect("static"),
        )
    });
    let lines: Vec<String> = if rust {
        crate::search::digest::live::live_code(src)
            .into_iter()
            .map(|(_, l)| l.to_string())
            .collect()
    } else {
        crate::search::digest::ts::code_lines(src)
    };
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    // (impl type, the depth its block opened at)
    let mut impls: Vec<(String, i32)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if rust {
            if let Some(c) = impl_re.captures(t) {
                impls.push((c[1].to_string(), depth));
            }
        }
        let public = if rust {
            t.starts_with("pub ") && !t.starts_with("pub(")
        } else {
            t.starts_with("export ") && !t.starts_with("export {") && !t.starts_with("export *")
        };
        if public {
            // the declaration, across lines (12 at most)
            let text: String = lines[i..lines.len().min(i + 12)].join("\n");
            // the declaration head alone names it: never a later item's fn
            let head = normalize_decl(&text, false);
            let (name, cut) = match fn_re.captures(&head) {
                Some(c) if rust || head.contains("function") => (c[1].to_string(), false),
                _ => match item_re.captures(&head) {
                    Some(c) => {
                        let kw = c[1].to_string();
                        (
                            c[2].to_string(),
                            matches!(kw.as_str(), "const" | "static" | "let" | "var"),
                        )
                    }
                    None => (String::new(), false),
                },
            };
            if !name.is_empty() {
                let key = match impls.last() {
                    Some((ty, at)) if rust && depth > *at => format!("{ty}::{name}"),
                    _ => name,
                };
                out.push((key, normalize_decl(&text, cut)));
            }
        }
        depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
        while impls
            .last()
            .is_some_and(|(_, at)| depth <= *at && !t.contains('{'))
        {
            impls.pop();
        }
        if let Some((_, at)) = impls.last() {
            if depth <= *at && t.contains('}') {
                impls.pop();
            }
        }
    }
    out
}
