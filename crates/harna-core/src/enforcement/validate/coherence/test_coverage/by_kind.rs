//! Component `tests-by-kind` (0.4.10, field test 049): a declared test
//! kind makes its files discoverable, not only reach-checked. An
//! integration test in `crates/store/tests/login_flow.rs` that names
//! `user_store` covers the `user-store` component — `harna tests` lists
//! it under its kind, and coverage counts it — without registering the
//! file on every component by hand.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use crate::schema::model::manifest::{Component, Manifest};

/// One file of a declared test kind and the components it names.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KindFile {
    pub kind: String,
    pub file: String,
    pub covers: Vec<String>,
}

/// The test-kind files outside every component's own file, with the
/// components each one names (by module name, file stem or declared
/// surface — word-bounded, comments included: a cheap, honest reading).
pub fn files(root: &Path, manifest: &Manifest) -> Vec<KindFile> {
    if manifest.test_kinds.is_empty() {
        return Vec::new();
    }
    let mut comps: Vec<&Component> = Vec::new();
    fn walk<'m>(cs: &'m [Component], out: &mut Vec<&'m Component>) {
        for c in cs {
            out.push(c);
            walk(&c.children, out);
        }
    }
    for l in &manifest.layers {
        for c in &l.containers {
            walk(&c.components, &mut comps);
        }
    }
    let own: BTreeSet<String> = comps
        .iter()
        .filter(|c| !c.path.is_empty())
        .map(|c| c.path.replace('\\', "/"))
        .collect();
    let words = |c: &Component| -> Vec<String> {
        let mut w = vec![c.name.replace('-', "_")];
        if let Some(stem) = Path::new(&c.path).file_stem().and_then(|s| s.to_str()) {
            if stem != "mod" && stem != "lib" {
                w.push(stem.to_string());
            }
        }
        w.extend(c.provides.iter().cloned());
        // the component's own public types name it too (0.5.9
        // field test: a ui test used the re-exported `LedgerView`, never
        // the module path)
        if let Ok(src) = std::fs::read_to_string(root.join(&c.path)) {
            for line in src.lines().map(str::trim) {
                let rest = line.strip_prefix("pub ").unwrap_or("");
                for kw in ["struct ", "enum ", "trait ", "type "] {
                    if let Some(after) = rest.strip_prefix(kw) {
                        let name: String = after
                            .chars()
                            .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                            .collect();
                        if name.len() > 2 {
                            w.push(name);
                        }
                    }
                }
            }
        }
        w
    };
    let mut out = Vec::new();
    for f in crate::search::digest::respected_rust_files(root, 100_000) {
        if !manifest.test_kinds.iter().any(|k| k.covers(&f)) {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(root.join(&f)) else {
            continue;
        };
        let Some(kind) = manifest.test_kinds.iter().find(|k| k.covers_file(&f, &src)) else {
            continue;
        };
        // a component's own file carrying the kind's marker (inline
        // `#[gpui_kit::test]` in the view, 0.5.9) covers that component
        if own.contains(&f) {
            out.push(KindFile {
                kind: kind.name.clone(),
                file: f.clone(),
                covers: comps
                    .iter()
                    .filter(|c| !c.planned && c.path.replace('\\', "/") == f)
                    .map(|c| c.name.clone())
                    .collect(),
            });
            continue;
        }
        let covers: Vec<String> = comps
            .iter()
            .filter(|c| !c.planned)
            .filter(|c| {
                words(c).iter().any(|w| {
                    regex::Regex::new(&format!(r"\b{}\b", regex::escape(w)))
                        .is_ok_and(|re| re.is_match(&src))
                })
            })
            .map(|c| c.name.clone())
            .collect();
        out.push(KindFile {
            kind: kind.name.clone(),
            file: f,
            covers,
        });
    }
    out
}

/// The components some test-kind file names.
pub fn covered(root: &Path, manifest: &Manifest) -> BTreeSet<String> {
    files(root, manifest)
        .into_iter()
        .flat_map(|k| k.covers)
        .collect()
}

pub fn render(files: &[KindFile]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut out = String::from("by test kind:\n");
    for k in files {
        out.push_str(&format!(
            "  [{}] {} — covers {}\n",
            k.kind,
            k.file,
            if k.covers.is_empty() {
                "no registered component by name".to_string()
            } else {
                k.covers.join(", ")
            }
        ));
    }
    out
}
