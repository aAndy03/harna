//! Component `tests`: registered tests earn their keep.
//!
//! The manifest's `tests` field turns test coverage into registered data.
//! Two checks:
//! - `check_declared` (whole repo): a registered test path exists and, when
//!   it is a Rust file, actually contains test code — the manifest never
//!   points at fiction.
//! - `check_changes` (changed-scope only): a component changed in this burst
//!   but none of its registered tests did. Body-only edits are governed by
//!   `validate --changed` (the route `digest::diff` routes `contents_changed`
//!   down), and this is their companion: the component-change signal comes
//!   from the same git-scoped burst, the test-update signal from the file's
//!   inline test region (working tree vs `HEAD`) or a touched sibling test
//!   file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::schema::model::manifest::{Component, Manifest};

use super::{Severity, Violation};

/// Whole-repo coherence: declared test paths exist and hold real tests.
pub fn check_declared(root: &Path, manifest: &Manifest, out: &mut Vec<Violation>) {
    for_each_component(manifest, &mut |comp| {
        for t in &comp.tests {
            let t = t.replace('\\', "/");
            let abs = root.join(&t);
            if !abs.exists() {
                out.push(Violation {
                    check: "tests-declared".into(),
                    file: t.clone(),
                    line: 0,
                    message: format!(
                        "component `{}` declares test path `{t}` that does not exist",
                        comp.name
                    ),
                    hint: "create the test file, or fix the declared path".into(),
                    severity: Severity::Error,
                });
                continue;
            }
            if !t.ends_with(".rs") {
                continue; // non-Rust test assets: existence is the contract for now
            }
            if let Ok(src) = std::fs::read_to_string(&abs) {
                if test_region(&src).is_empty() {
                    out.push(Violation {
                        check: "tests-declared".into(),
                        file: t.clone(),
                        line: 0,
                        message: format!(
                            "component `{}` declares `{t}` as a test, but the file contains no test code",
                            comp.name
                        ),
                        hint: "add the tests that cover this component, or declare the real ones".into(),
                        severity: Severity::Info,
                    });
                }
            }
        }
    });
}

/// Changed-scope check (the `validate --changed` enforcement loop): every
/// changed component that registers tests must have touched at least one of
/// them in the same burst. Advisory: a burst that needs no new tests is
/// legitimate — record why, visibly.
pub fn check_changes(
    root: &Path,
    manifest: &Manifest,
    changed: &[String],
    out: &mut Vec<Violation>,
) {
    for_each_component(manifest, &mut |comp| {
        if comp.tests.is_empty() || !changed.contains(&comp.path) {
            return;
        }
        if comp
            .tests
            .iter()
            .any(|t| test_was_updated(root, comp, t, changed))
        {
            return;
        }
        out.push(Violation {
            check: "no-test-update".into(),
            file: comp.path.clone(),
            line: 0,
            severity: Severity::Info,
            message: format!(
                "component `{}` changed, but none of its registered tests did ({})",
                comp.name,
                comp.tests.join(", ")
            ),
            hint: "extend the registered tests with the new behavior, or record in the decision log why this change needs none".into(),
        });
    });
}

/// Did registered test `test` change in this burst? Inline tests (the
/// component's own file) count as updated when the file's test region differs
/// from `HEAD`; separate test files when the file itself is in the burst.
fn test_was_updated(root: &Path, comp: &Component, test: &str, changed: &[String]) -> bool {
    let t = test.replace('\\', "/");
    if t != comp.path {
        return changed.contains(&t);
    }
    let now = std::fs::read_to_string(root.join(&t)).map(|s| test_region(&s).to_string());
    match head_version(root, &t).map(|s| test_region(&s).to_string()) {
        Some(baseline) => {
            let current = now.unwrap_or_default();
            current != baseline
        }
        // Untracked/new file: presence of test code is the update signal; the
        // no-test-code case is `check_declared`'s to flag.
        None => now.map(|s| !s.is_empty()).unwrap_or(false),
    }
}

/// Walk every component, including nested children.
fn for_each_component(manifest: &Manifest, f: &mut impl FnMut(&Component)) {
    for layer in &manifest.layers {
        for container in &layer.containers {
            walk(&container.components, f);
        }
    }
}

/// Coverage ratio as data (0.3.11): how many components register tests.
/// `percent` floors (88.9 -> 88 — the line never flatters); an empty map is
/// vacuously 100 (nothing to cover, nothing uncovered).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    pub total: usize,
    pub with_tests: usize,
    pub percent: u8,
    pub untested: Vec<String>,
}

pub fn coverage(manifest: &Manifest) -> Coverage {
    coverage_with(manifest, &Default::default())
}

/// Coverage where `also` (components a test-kind file names, 0.4.10)
/// count as tested beside the registered ones.
pub fn coverage_with(manifest: &Manifest, also: &std::collections::BTreeSet<String>) -> Coverage {
    let mut total = 0usize;
    let mut with_tests = 0usize;
    let mut untested = Vec::new();
    for_each_component(manifest, &mut |comp| {
        if comp.planned {
            return; // a slot has no code to test yet (0.4.9)
        }
        total += 1;
        if comp.tests.is_empty() && !also.contains(&comp.name) {
            untested.push(comp.name.clone());
        } else {
            with_tests += 1;
        }
    });
    untested.sort();
    // Nothing declared is vacuously covered.
    let percent = (with_tests * 100)
        .checked_div(total)
        .map_or(100, |p| p.min(100) as u8);
    Coverage {
        total,
        with_tests,
        percent,
        untested,
    }
}

/// One registered test mapped to the `cargo test` invocation that runs
/// it (0.3.11). The mapping is structural — path shape to command shape —
/// never a claim the target passes; `tests-declared` (existence + real
/// test code) and actually running the command are the verification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestTarget {
    /// Component that registers the test.
    pub component: String,
    /// Registered path, as declared in the manifest.
    pub test: String,
    /// Runnable invocation, or empty when the path maps to no cargo target.
    pub command: String,
    /// What the command selects, or why the path is unmappable.
    pub note: String,
}

impl TestTarget {
    pub fn runnable(&self) -> bool {
        !self.command.is_empty()
    }
}

/// Map every registered test to its runner. Crate membership resolves
/// through the workspace's own Cargo.tomls (the same honest-toml read
/// `crate_name_dirs` does); a path under no known crate dir stays
/// unmappable rather than guessing a package.
pub fn runnable(root: &Path, manifest: &Manifest) -> Vec<TestTarget> {
    let dirs: Vec<String> = {
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for_each_component(manifest, &mut |comp| {
            for t in &comp.tests {
                let t = t.replace('\\', "/");
                if let Some(dir) = crate_dir_of(&t) {
                    if seen.insert(dir.clone()) {
                        out.push(dir);
                    }
                }
            }
        });
        out
    };
    let name_dirs = crate::search::digest::crate_name_dirs(root, &dirs);
    let mut dir_to_pkg: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for (pkg, dir) in &name_dirs {
        dir_to_pkg.entry(dir.as_str()).or_insert(pkg.as_str());
    }
    let mut targets = Vec::new();
    for_each_component(manifest, &mut |comp| {
        for t in &comp.tests {
            let t = t.replace('\\', "/");
            targets.push(map_one(comp.name.as_str(), &t, &dir_to_pkg));
        }
    });
    targets
}

fn map_one(
    component: &str,
    test: &str,
    dir_to_pkg: &std::collections::BTreeMap<&str, &str>,
) -> TestTarget {
    let base = |command: String, note: &str| TestTarget {
        component: component.into(),
        test: test.into(),
        command,
        note: note.into(),
    };
    if !test.ends_with(".rs") {
        return base(
            String::new(),
            "non-Rust test asset: existence is the contract, no cargo target",
        );
    }
    let Some(dir) = crate_dir_of(test) else {
        return base(
            String::new(),
            "outside any workspace crate dir (`crates/<name>/…`): no package to address",
        );
    };
    let Some(pkg) = dir_to_pkg.get(dir.as_str()) else {
        return base(
            String::new(),
            "crate dir has no readable package name: add a [package] name to its Cargo.toml",
        );
    };
    let rel = test.strip_prefix(&format!("{dir}/")).unwrap_or(test);
    // Integration tests and benches are cargo targets by file stem.
    if let Some(rest) = rel.strip_prefix("tests/") {
        let stem = rest.trim_end_matches(".rs");
        if stem.is_empty() || stem.contains('/') {
            return base(
                String::new(),
                "integration tests live directly in `tests/*.rs`",
            );
        }
        return base(
            format!("cargo test -p {pkg} --test {stem}"),
            "integration target: runs the file's tests",
        );
    }
    if let Some(rest) = rel.strip_prefix("benches/") {
        let stem = rest.trim_end_matches(".rs");
        if stem.is_empty() || stem.contains('/') {
            return base(String::new(), "benches live directly in `benches/*.rs`");
        }
        return base(
            format!("cargo test -p {pkg} --bench {stem}"),
            "bench target: runs the file's tests",
        );
    }
    // Unit tests: module filter under `src/`.
    let Some(src_rel) = rel.strip_prefix("src/") else {
        return base(
            String::new(),
            "not under `src/` (and not tests/ or benches/): no target shape applies",
        );
    };
    if src_rel == "main.rs" || src_rel == "lib.rs" || src_rel.starts_with("bin/") {
        return base(
            format!("cargo test -p {pkg}"),
            "crate root/binary target: no module filter, runs the whole package",
        );
    }
    let stem = src_rel.trim_end_matches(".rs");
    let module = stem.strip_suffix("/mod").unwrap_or(stem).replace('/', "::");
    if module.is_empty() {
        return base(
            format!("cargo test -p {pkg}"),
            "crate root module: runs the whole package",
        );
    }
    base(
        format!("cargo test -p {pkg} --lib {module}"),
        "lib module filter: runs the file's inline tests",
    )
}

/// The workspace crate dir a path lives in: `crates/<name>` (two segments)
/// or the root crate `.` for top-level `src/…` paths. Anything else (docs,
/// scripts, absolute strays) is no crate at all.
fn crate_dir_of(path: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix("crates/") {
        let mut segs = rest.split('/');
        let (head, next) = (segs.next()?, segs.next()?);
        if head.is_empty() || next.is_empty() {
            return None;
        }
        return Some(format!("crates/{head}"));
    }
    if path.starts_with("src/") {
        return Some(".".into());
    }
    None
}

/// Token-efficient rendering of the mapping for humans and agents.
pub fn render_targets(targets: &[TestTarget]) -> String {
    let runnable_count = targets.iter().filter(|t| t.runnable()).count();
    let mut out = format!(
        "registered tests: {} target(s), {} runnable via cargo test\n",
        targets.len(),
        runnable_count
    );
    for t in targets {
        if t.runnable() {
            out.push_str(&format!(
                "  {} — `{}`\n    run: {}\n",
                t.component, t.test, t.command
            ));
        } else {
            out.push_str(&format!(
                "  {} — `{}`\n    run: — ({})\n",
                t.component, t.test, t.note
            ));
        }
    }
    out
}

fn walk<'a>(comps: &'a [Component], f: &mut impl FnMut(&'a Component)) {
    for c in comps {
        f(c);
        walk(&c.children, f);
    }
}

/// Does a line open a test section?
fn is_test_marker(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("#[test]")
        || t.starts_with("#[cfg(test)]")
        || t.starts_with("#[tokio::test]")
        || t.starts_with("#[rstest]")
}

/// Everything from the first test marker to the end of the file — the region
/// an inline test update would touch.
fn test_region(src: &str) -> &str {
    let mut offset = 0usize;
    for line in src.split_inclusive('\n') {
        if is_test_marker(line) {
            return &src[offset..];
        }
        offset += line.len();
    }
    ""
}

/// The file at `HEAD`, or None when absent (untracked/new).
fn head_version(root: &Path, rel: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["show", &format!("HEAD:{rel}")])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub mod by_kind;

#[cfg(test)]
mod tests;
