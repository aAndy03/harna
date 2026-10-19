//! `unreferenced-snapshot` (0.3.48, R5): test data whose test is gone.
//! ruff keeps 3,734 insta `.snap` files and codex 1,599; CI rejects the
//! unreferenced ones by running the suite (`cargo insta test --unreferenced
//! reject`). harna runs no code, so it reads the one thing the file name
//! proves: insta prefixes every snapshot with the producing module's path
//! (`ruff_linter__rules__pyflakes__tests__F401_0.py.snap` came from
//! `ruff_linter::rules::pyflakes::tests`). When the map knows no such
//! module — not as a file, not as an inline `mod` of the deepest file that
//! does exist — the snapshot outlived its test.
//!
//! Snapshot names are often built at run time (`format!`), so a snapshot
//! is never judged by its name, only by its module. Advisory until a rule
//! binds the check (`adopted_by_binding`).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::Violation;
use crate::enforcement::validate::hygiene::finding;
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::path_within;
use crate::search::digest::crates::CrateIndex;
use crate::search::digest::Digest;

pub fn check(
    root: &Path,
    rules: &Rules,
    env: Option<&str>,
    digest: Option<&Digest>,
    out: &mut Vec<Violation>,
) {
    let Some(digest) = digest else {
        return; // no map: no modules to judge by
    };
    let snaps = crate::search::digest::respected_files_where(root, 200_000, |p| {
        p.ends_with(".snap") && p.contains("/snapshots/")
    });
    if snaps.is_empty() {
        return;
    }
    let scopes: Vec<String> = rules
        .rules
        .iter()
        .filter(|r| {
            !r.is_deprecated()
                && r.matches_env(env)
                && r.check.as_deref() == Some("unreferenced-snapshot")
        })
        .flat_map(|r| r.scope.clone())
        .collect();
    let modules = module_paths(root, digest);
    for snap in snaps {
        if !scopes.is_empty() && !scopes.iter().any(|s| path_within(&snap, s)) {
            continue;
        }
        if let Some(module) = gone(&snap, &modules, root) {
            out.push(finding(
                "unreferenced-snapshot",
                &snap,
                0,
                format!(
                    "snapshot of `{module}`, a module that no longer exists — its test is gone"
                ),
                "delete it (or `cargo insta test --unreferenced delete`), or restore the test"
                    .into(),
            ));
        }
    }
}

/// Every module path the map knows (`crate::a::b`, crate spelled as Rust
/// imports it), with the files that define it (a crate's `lib.rs` and
/// `main.rs` are both its root).
fn module_paths(root: &Path, digest: &Digest) -> HashMap<String, Vec<String>> {
    let index = CrateIndex::load(root);
    let mut idents: HashMap<&str, Vec<String>> = HashMap::new();
    for (name, package) in &index.import_names {
        idents
            .entry(package.as_str())
            .or_default()
            .push(name.clone());
    }
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for m in &digest.modules {
        let Some((c, stem)) = index.locate(&m.path) else {
            continue;
        };
        let stem = stem.trim_end_matches("/mod");
        // An integration test, bench or example is a crate of its own,
        // named after its file.
        let rel = m
            .path
            .strip_prefix(&format!("{}/", c.dir))
            .unwrap_or(&m.path);
        let own_target = ["tests/", "benches/", "examples/"]
            .iter()
            .find_map(|a| rel.strip_prefix(a))
            .and_then(|r| r.strip_suffix(".rs"))
            .filter(|r| !r.contains('/'));
        if let Some(target) = own_target {
            out.entry(target.to_string())
                .or_default()
                .push(m.path.clone());
            continue;
        }
        let inner = if m.is_crate_root() {
            String::new()
        } else {
            format!("::{}", stem.replace('/', "::"))
        };
        for ident in idents.get(c.package.as_str()).into_iter().flatten() {
            out.entry(format!("{ident}{inner}"))
                .or_default()
                .push(m.path.clone());
        }
    }
    out
}

/// The producing module of a snapshot when the map proves it gone.
fn gone(snap: &str, modules: &HashMap<String, Vec<String>>, root: &Path) -> Option<String> {
    let file = snap.rsplit('/').next()?.strip_suffix(".snap")?;
    let parts: Vec<&str> = file.split("__").collect();
    if parts.len() < 2 || !modules.contains_key(parts[0]) {
        return None; // not a module-prefixed insta name: no verdict
    }
    let module = &parts[..parts.len() - 1];
    // the longest prefix the map knows as a file
    let known = (1..=module.len())
        .rev()
        .find(|&k| modules.contains_key(&module[..k].join("::")))?;
    if known == module.len() {
        return None;
    }
    // the next segment may be an inline (or `#[path]`) module of that
    // file: `mod tests {`, `#[path = "x_tests.rs"] mod tests;`
    let next = module[known];
    let declared = modules[&module[..known].join("::")].iter().any(|host| {
        std::fs::read_to_string(root.join(host)).is_ok_and(|src| inline_mods(&src).contains(next))
    });
    if declared || named_in_source(snap, file, root) {
        return None;
    }
    Some(module.join("::"))
}

/// A snapshot named explicitly (`assert_snapshot!("codex_core__codex_tests__…")`)
/// is referenced by that string in a source beside its `snapshots/` folder
/// (codex names some with a module-like prefix that is not its module).
fn named_in_source(snap: &str, name: &str, root: &Path) -> bool {
    let Some((dir, _)) = snap.rsplit_once("/snapshots/") else {
        return false;
    };
    let quoted = format!("\"{name}\"");
    crate::search::digest::respected_files_where(root, 50_000, |p| {
        p.ends_with(".rs") && p.starts_with(&format!("{dir}/"))
    })
    .iter()
    .any(|f| std::fs::read_to_string(root.join(f)).is_ok_and(|src| src.contains(&quoted)))
}

/// Names of every `mod NAME` in a source, inline or not.
fn inline_mods(src: &str) -> HashSet<String> {
    src.lines()
        .filter_map(|l| {
            let mut t = l.trim_start();
            while t.starts_with("#[") {
                t = t.split_once(']').map(|(_, r)| r.trim_start()).unwrap_or("");
            }
            if t.starts_with("pub(") {
                t = t.split_once(')').map(|(_, r)| r.trim_start()).unwrap_or(t);
            }
            let t = t.strip_prefix("pub ").unwrap_or(t);
            let rest = t.strip_prefix("mod ")?;
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

#[cfg(test)]
mod tests;
