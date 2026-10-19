//! Component `reuse` (0.3.8): duplication made visible.
//!
//! - `no-copy-paste`: near-duplicate blocks across files, named. A block
//!   is a run of 5 consecutive non-trivial lines, normalized (comments,
//!   whitespace, literals stripped); a shared normalized run in two or
//!   more files is a copy — the containing files are named explicitly.
//!
//! The check reads the LIVE sources of the run's file set (the digest
//! only stores signatures, never bodies), so `--changed` scopes it to
//! the changed pair and a full run compares the whole repo.
//! - `single-owner`: one symbol, one home — a declared name in two or
//!   more modules is a colliding home; the digest inventory (pub AND
//!   private) is the witness. Deliberately advisory by default so
//!   legitimate "two similar things in different containers" naming
//!   stays visible rather than suddenly blocking the build.

use std::collections::BTreeMap;
use std::hash::{Hash as _, Hasher as _};

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::search::digest::Digest;

const SHINGLE: usize = 5;
const DISPLAY_RUNS: usize = 3;

/// Strip what makes two blocks differ cosmetically: comments, blank
/// lines, string/char literal contents and numeric literals — so two
/// hand-edited copies still match while real divergences do not.
fn normalized_lines(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_test_code = false;
    for line in src.lines() {
        let t = line.trim();
        if !in_test_code
            && (t.starts_with("#[cfg(test)]")
                || t.starts_with("#[test]")
                || t.starts_with("mod tests"))
        {
            in_test_code = true;
        }
        if in_test_code || t.is_empty() {
            continue;
        }
        let mut s = t.to_string();
        if s.starts_with("//") || s.starts_with("///") || s.starts_with("//!") {
            continue;
        }
        s = strip_literals(&s);
        s = s
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        let mut tokens: Vec<&str> = s.split_whitespace().collect();
        tokens.dedup();
        if tokens.is_empty() {
            continue;
        }
        out.push(tokens.join(" ").to_lowercase());
    }
    out
}

fn strip_literals(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    let mut in_lit: Option<char> = None;
    for c in chars.by_ref() {
        match in_lit {
            Some(q) => {
                if c == q {
                    in_lit = None;
                }
                // everything inside the literal disappears: two copies that
                // differ only in link/name literals still match
            }
            None => match c {
                '\'' | '"' => in_lit = Some(c),
                _ => out.push(c),
            },
        }
    }
    out
}

/// A block shared by more files than this is a convention (boilerplate,
/// generated scaffolding), not a copy — and pairing every file that holds
/// it was quadratic: 35s of a 45s validate on codex's 5,353 modules
/// (0.3.46).
const BOILERPLATE: usize = 10;

fn shingle_hashes(lines: &[String], file: u32, out: &mut std::collections::HashMap<u64, Vec<u32>>) {
    if lines.len() < SHINGLE {
        return;
    }
    for w in lines.windows(SHINGLE) {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        w.hash(&mut h);
        let entry = out.entry(h.finish()).or_default();
        if entry.len() <= BOILERPLATE && entry.last() != Some(&file) {
            entry.push(file);
        }
    }
}

pub fn check_copy_paste(srcs: &[(String, String)], out: &mut Vec<Violation>) {
    // File indices, not path clones, in the hot maps (0.3.46: 39s on codex).
    let mut shingles: std::collections::HashMap<u64, Vec<u32>> = std::collections::HashMap::new();
    for (i, (_, src)) in srcs.iter().enumerate() {
        shingle_hashes(&normalized_lines(src), i as u32, &mut shingles);
    }
    // A file pair shares how many distinct runs? Accumulate per unordered pair.
    let mut counts: std::collections::HashMap<(u32, u32), usize> = std::collections::HashMap::new();
    for files in shingles.values() {
        if files.len() < 2 || files.len() > BOILERPLATE {
            continue;
        }
        for i in 0..files.len() {
            for j in i + 1..files.len() {
                *counts.entry((files[i], files[j])).or_default() += 1;
            }
        }
    }
    let mut pairs: BTreeMap<(String, String), usize> = BTreeMap::new();
    for ((i, j), c) in counts {
        let (a, b) = (&srcs[i as usize].0, &srcs[j as usize].0);
        let key = if a <= b {
            (a.clone(), b.clone())
        } else {
            (b.clone(), a.clone())
        };
        pairs.insert(key, c);
    }
    for ((a, b), count) in pairs {
        if count < 2 {
            // One shared 5-line truth: often a serial id or a pragma.
            continue;
        }
        out.push(Violation {
            check: "no-copy-paste".into(),
            file: a.clone(),
            line: 0,
            severity: severity_for("no-copy-paste"),
            message: format!(
                "`{a}` and `{b}` share {} normalized block(s) — one of them was probably copied",
                count,
            ),
            hint: "reading side by side, decide the single true home and edit both; if the blocks diverge on purpose, move the shared part behind one fn".to_string(),
        });
    }
}

pub fn check_single_owner(digest: Option<&Digest>, out: &mut Vec<Violation>) {
    let Some(d) = digest else { return };
    // "One symbol, one home" is a within-crate discipline: the same type
    // name in two crates (their own request DTOs, adapters) is normal
    // namespacing, and function/impl names repeat idiomatically across
    // containers. Types (struct | enum | trait) declared twice in ONE
    // crate are the confusion the check sees.
    let mut homes: BTreeMap<(String /*crate*/, String /*name*/), Vec<String>> = BTreeMap::new();
    for m in &d.modules {
        let (crate_dir, _) = m.crate_of();
        let crate_key = crate_dir.clone().unwrap_or_else(|| ".".into());
        for i in &m.inventory {
            if i.vis == "priv" || !matches!(i.kind.as_str(), "struct" | "enum" | "trait") {
                continue;
            }
            homes
                .entry((crate_key.clone(), i.name.clone()))
                .or_default()
                .push(m.path.clone());
        }
    }
    for ((_, name), files) in homes {
        let homes_listed: std::collections::BTreeSet<String> = files.into_iter().collect();
        if homes_listed.len() < 2 {
            continue;
        }
        out.push(Violation {
            check: "single-owner".into(),
            file: homes_listed.iter().next().unwrap().clone(),
            line: 0,
            severity: severity_for("single-owner"),
            message: format!(
                "type `{name}` is declared in {} home(s) in the same crate: {} — one symbol, one home",
                homes_listed.len(),
                homes_listed.iter().take(DISPLAY_RUNS).cloned().collect::<Vec<_>>().join(", ")
            ),
            hint: "rename one, or make the distinct ones explicit with different names (the map now names the collision)".to_string(),
        });
    }
}

fn severity_for(check_id: &str) -> Severity {
    checks::spec(check_id)
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::digest::Module;

    #[test]
    fn near_duplicate_blocks_are_named_with_both_files() {
        let copied = "pub fn compute(m: Vec<Node>) -> u32 {\n    let mut out = 0;\n    for node in m {\n        if node.kind == NodeKind::Leaf {\n            out += node.weight;\n        } else {\n            out += compute(node.children_loc);\n        }\n    }\n    out + m.head().count()\n}\n";
        let srcs = vec![
            ("src/a.rs".to_string(), copied.to_string()),
            (
                "src/b.rs".to_string(),
                copied.replacen("compute", "process", 1),
            ),
            (
                "src/clean.rs".to_string(),
                "pub fn others() {}\nlet x = 1;\n".to_string(),
            ),
        ];
        let mut v = Vec::new();
        check_copy_paste(&srcs, &mut v);
        assert_eq!(v.len(), 1, "one duplicated pair, was {:#?}", v.len());
        assert!(v[0].message.contains("src/a.rs") && v[0].message.contains("src/b.rs"));
        // Name edits inside literals do not dodge the detector: the copies
        // stay near-duplicates by construction.
    }

    #[test]
    fn different_code_and_shared_pragmas_stay_silent() {
        let srcs = vec![
            (
                "src/x.rs".to_string(),
                "pub struct A;\npub fn f(a: u8) -> u8 { a * 3 }\n".to_string(),
            ),
            (
                "src/y.rs".to_string(),
                "pub struct B;\npub fn g(b: u16) -> u16 { b * 7 }\n".to_string(),
            ),
        ];
        let mut v = Vec::new();
        check_copy_paste(&srcs, &mut v);
        assert!(v.is_empty(), "short files and distinct code are not copies");
    }

    #[test]
    fn one_home_is_quiet_collisions_are_named() {
        let d = Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                crate::search::digest::Module {
                    reexports: Vec::new(),
                    crate_dir: None,
                    src_root: None,
                    path: "crates/t/src/a.rs".into(),
                    lang: "rust".into(),
                    signatures: vec![],
                    deps: vec![],
                    body_hash: "x".into(),
                    inventory: vec![crate::search::digest::Item {
                        vis: "pub".into(),
                        kind: "struct".into(),
                        name: "Renderer".into(),
                        text: "pub struct Renderer".into(),
                    }],
                    mods: vec![],
                },
                crate::search::digest::Module {
                    reexports: Vec::new(),
                    crate_dir: None,
                    src_root: None,
                    path: "crates/t/src/b.rs".into(),
                    lang: "rust".into(),
                    signatures: vec![],
                    deps: vec![],
                    body_hash: "x".into(),
                    inventory: vec![
                        crate::search::digest::Item {
                            vis: "pub".into(),
                            kind: "struct".into(),
                            name: "Renderer".into(),
                            text: "pub struct Renderer".into(),
                        },
                        crate::search::digest::Item {
                            vis: "priv".into(),
                            kind: "fn".into(),
                            name: "Renderer_2".into(),
                            text: "fn Renderer_2() {}".into(),
                        },
                    ],
                    mods: vec![],
                },
                Module {
                    reexports: Vec::new(),
                    crate_dir: None,
                    src_root: None,
                    path: "crates/t/src/c.rs".into(),
                    lang: "rust".into(),
                    signatures: vec![],
                    deps: vec![],
                    body_hash: "x".into(),
                    inventory: vec![],
                    mods: vec![],
                },
            ],
        };
        let mut v = Vec::new();
        check_single_owner(Some(&d), &mut v);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("type `Renderer`"));
        assert!(v[0].message.contains("a.rs") && v[0].message.contains("b.rs"));
        assert_eq!(v[0].severity, Severity::Info, "advisory by default");
    }

    #[test]
    fn same_names_across_crates_and_fn_idioms_are_normal_namespacing() {
        let d = Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                crate::search::digest::Module {
                    reexports: Vec::new(),
                    crate_dir: None,
                    src_root: None,
                    path: "crates/core/src/a.rs".into(),
                    lang: "rust".into(),
                    signatures: vec![],
                    deps: vec![],
                    body_hash: "x".into(),
                    inventory: vec![crate::search::digest::Item {
                        vis: "pub".into(),
                        kind: "struct".into(),
                        name: "Request".into(),
                        text: "pub struct Request".into(),
                    }],
                    mods: vec![],
                },
                crate::search::digest::Module {
                    reexports: Vec::new(),
                    crate_dir: None,
                    src_root: None,
                    path: "crates/mcp/src/b.rs".into(),
                    lang: "rust".into(),
                    signatures: vec![],
                    deps: vec![],
                    body_hash: "x".into(),
                    inventory: vec![
                        crate::search::digest::Item {
                            vis: "pub".into(),
                            kind: "struct".into(),
                            name: "Request".into(),
                            text: "pub struct Request".into(),
                        },
                        crate::search::digest::Item {
                            vis: "pub".into(),
                            kind: "fn".into(),
                            name: "check".into(),
                            text: "pub fn check()".into(),
                        },
                    ],
                    mods: vec![],
                },
                crate::search::digest::Module {
                    reexports: Vec::new(),
                    crate_dir: None,
                    src_root: None,
                    path: "crates/core/src/c.rs".into(),
                    lang: "rust".into(),
                    signatures: vec![],
                    deps: vec![],
                    body_hash: "x".into(),
                    inventory: vec![crate::search::digest::Item {
                        vis: "pub".into(),
                        kind: "fn".into(),
                        name: "check".into(),
                        text: "pub fn check()".into(),
                    }],
                    mods: vec![],
                },
            ],
        };
        let mut v = Vec::new();
        check_single_owner(Some(&d), &mut v);
        assert!(
            v.is_empty(),
            "each crate owns its namespace and fn names repeat idiomatically: {:#?}",
            v
        );
    }

    /// codex (0.3.46): a block in a dozen files is boilerplate, not a copy —
    /// and pairing all of them was quadratic. Two files sharing it still name.
    #[test]
    fn blocks_shared_by_many_files_are_conventions_not_copies() {
        let block = "let a = one();
let b = two(a);
let c = three(b);
let d = four(c);
let e = five(d);
let f = six(e);
let g = seven(f);
";
        let many: Vec<(String, String)> = (0..12)
            .map(|i| (format!("src/m{i}.rs"), block.to_string()))
            .collect();
        let mut v = Vec::new();
        check_copy_paste(&many, &mut v);
        assert!(v.is_empty(), "{v:?}");
        let mut v = Vec::new();
        check_copy_paste(&many[..2], &mut v);
        assert_eq!(v.len(), 1);
    }
}
