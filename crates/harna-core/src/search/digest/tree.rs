//! `tree` (0.3.47, zed/ruff study H10): which `.rs` files are source.
//!
//! A file is source when a crate target's module tree reaches it: from the
//! crate root (`lib`, `main`, or the file named after the crate), the
//! integration tests, benches and examples (`tests/*.rs`, …), binaries in
//! `src/bin/`, and `build.rs`, following `mod` declarations — test-gated
//! ones included. Anything else is data: zed's eval fixtures
//! (`crates/agent/src/tools/evals/fixtures/*/before.rs`) are Rust text no
//! compiler reads, and checking them invented surface errors.
//!
//! `#[path = "…"]` modules are followed from the declaring file's directory.
//! Files no declared crate hosts are left alone (unknown, not orphaned), and
//! so is every file of a crate whose roots the scan does not see (a custom
//! `[lib] path`, a truncated scan): no tree, no verdict.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use super::crates::CrateIndex;
use super::Module;

pub mod decls;

/// Paths of modules no crate target reaches. The index gives the paths
/// Cargo declares as target roots and the packages no workspace lists.
pub fn orphans(modules: &[Module], index: &CrateIndex) -> HashSet<String> {
    let targets = &index.targets;
    let mut by_stem: ByStem = HashMap::new();
    let by_path: HashMap<&str, &Module> = modules.iter().map(|m| (m.path.as_str(), m)).collect();
    for m in modules {
        let (dir, stem) = m.crate_of();
        if let Some(dir) = dir {
            by_stem.entry((dir, stem)).or_insert(m);
        }
    }
    let mut seen: HashSet<&str> = HashSet::new();
    // Two phases: declared and conventional roots first; a file named after
    // its crate is a root only when nothing reached it as a `mod` (zed's
    // `src/zed.rs` is `mod zed;` of `main.rs`, its children in `src/zed/`).
    let mut roots: HashSet<&str> = HashSet::new();
    for phase in [Root::Declared, Root::Named] {
        let mut queue: VecDeque<&Module> = VecDeque::new();
        for m in modules {
            if root_kind(m, targets) == Some(phase) && !seen.contains(m.path.as_str()) {
                roots.insert(m.path.as_str());
                queue.push_back(m);
            }
        }
        walk(queue, (&by_stem, &by_path), &roots, &mut seen);
    }
    let rooted: HashSet<&str> = modules
        .iter()
        .filter(|m| root_kind(m, targets).is_some())
        .filter_map(|m| m.crate_dir.as_deref())
        .collect();
    modules
        .iter()
        .filter(|m| {
            m.crate_dir.as_deref().is_some_and(|d| rooted.contains(d))
                && !seen.contains(m.path.as_str())
                && !in_foreign_package(m, &index.foreign)
        })
        .map(|m| m.path.clone())
        .collect()
}

/// Modules by (crate dir, module stem).
type ByStem<'a> = HashMap<(String, String), &'a Module>;

/// Breadth-first along `mod` declarations from `queue`.
fn walk<'a>(
    mut queue: VecDeque<&'a Module>,
    (by_stem, by_path): (&ByStem<'a>, &HashMap<&str, &'a Module>),
    roots: &HashSet<&str>,
    seen: &mut HashSet<&'a str>,
) {
    while let Some(m) = queue.pop_front() {
        if !seen.insert(m.path.as_str()) {
            continue;
        }
        let (Some(dir), stem) = m.crate_of() else {
            continue;
        };
        // A root's children sit beside it; a module's in its own folder.
        let base = if roots.contains(m.path.as_str()) {
            stem.rsplit_once('/')
                .map(|(d, _)| d.to_string())
                .unwrap_or_default()
        } else {
            stem.clone()
        };
        for child in &m.mods {
            if child.ends_with(".rs") {
                // `#[path]`: relative to the declaring file's directory
                let here = m.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                if let Some(c) = by_path.get(super::crates::resolve_rel(here, child).as_str()) {
                    queue.push_back(c);
                }
                continue;
            }
            let child_stem = if base.is_empty() {
                child.clone()
            } else {
                format!("{base}/{child}")
            };
            if let Some(c) = by_stem.get(&(dir.clone(), child_stem)) {
                queue.push_back(c);
            }
        }
    }
}

/// Whether `m` sits in an unlisted package nested below its crate dir.
fn in_foreign_package(m: &Module, foreign: &BTreeSet<String>) -> bool {
    let dir = m.crate_dir.as_deref().unwrap_or(".");
    let mut here = m.path.as_str();
    while let Some((parent, _)) = here.rsplit_once('/') {
        if parent == dir || (dir != "." && !parent.starts_with(dir)) {
            return false;
        }
        if foreign.contains(parent) {
            return true;
        }
        here = parent;
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Root {
    /// declared in Cargo.toml, `lib.rs`/`main.rs`, or a file Cargo compiles
    /// on its own (integration test, bench, example, binary, build script)
    Declared,
    /// the file named after its crate — a root unless a `mod` reaches it
    Named,
}

/// Whether (and how surely) `m` roots a compilation target.
fn root_kind(m: &Module, targets: &BTreeSet<String>) -> Option<Root> {
    if m.crate_dir.is_none() || targets.contains(&m.path) {
        return Some(Root::Declared);
    }
    let (_, stem) = m.crate_of();
    if stem == "lib" || stem == "main" {
        return Some(Root::Declared);
    }
    if m.is_crate_root() {
        return Some(Root::Named);
    }
    let dir = m.crate_dir.as_deref().unwrap_or(".");
    let rel = if dir == "." {
        m.path.as_str()
    } else {
        m.path.strip_prefix(&format!("{dir}/")).unwrap_or(&m.path)
    };
    let segs: Vec<&str> = rel.split('/').collect();
    let own = match segs.as_slice() {
        ["build.rs"] => true,
        [area, _] | [area, _, "main.rs"] => {
            matches!(*area, "tests" | "benches" | "examples")
        }
        ["src", "bin", _] | ["src", "bin", _, "main.rs"] => true,
        _ => false,
    };
    own.then_some(Root::Declared)
}

#[cfg(test)]
mod tests;
