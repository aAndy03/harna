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

use crate::schema::manifest::{Component, Manifest};

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
        if comp.tests.is_empty() || !changed.iter().any(|f| *f == comp.path) {
            return;
        }
        if comp.tests.iter().any(|t| test_was_updated(root, comp, t, changed)) {
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
        return changed.iter().any(|f| *f == t);
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
    let mut total = 0usize;
    let mut with_tests = 0usize;
    let mut untested = Vec::new();
    for_each_component(manifest, &mut |comp| {
        total += 1;
        if comp.tests.is_empty() {
            untested.push(comp.name.clone());
        } else {
            with_tests += 1;
        }
    });
    untested.sort();
    let percent = if total == 0 {
        100
    } else {
        (with_tests * 100 / total).min(100) as u8
    };
    Coverage { total, with_tests, percent, untested }
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

fn map_one(component: &str, test: &str, dir_to_pkg: &std::collections::BTreeMap<&str, &str>) -> TestTarget {
    let base = |command: String, note: &str| TestTarget {
        component: component.into(),
        test: test.into(),
        command,
        note: note.into(),
    };
    if !test.ends_with(".rs") {
        return base(String::new(), "non-Rust test asset: existence is the contract, no cargo target");
    }
    let Some(dir) = crate_dir_of(test) else {
        return base(String::new(), "outside any workspace crate dir (`crates/<name>/…`): no package to address");
    };
    let Some(pkg) = dir_to_pkg.get(dir.as_str()) else {
        return base(String::new(), "crate dir has no readable package name: add a [package] name to its Cargo.toml");
    };
    let rel = test.strip_prefix(&format!("{dir}/")).unwrap_or(test);
    // Integration tests and benches are cargo targets by file stem.
    if let Some(rest) = rel.strip_prefix("tests/") {
        let stem = rest.trim_end_matches(".rs");
        if stem.is_empty() || stem.contains('/') {
            return base(String::new(), "integration tests live directly in `tests/*.rs`");
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
        return base(String::new(), "not under `src/` (and not tests/ or benches/): no target shape applies");
    };
    if src_rel == "main.rs" || src_rel == "lib.rs" || src_rel.starts_with("bin/") {
        return base(
            format!("cargo test -p {pkg}"),
            "crate root/binary target: no module filter, runs the whole package",
        );
    }
    let stem = src_rel.trim_end_matches(".rs");
    let module = stem
        .strip_suffix("/mod")
        .unwrap_or(stem)
        .replace('/', "::");
    if module.is_empty() {
        return base(format!("cargo test -p {pkg}"), "crate root module: runs the whole package");
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
            out.push_str(&format!("  {} — `{}`\n    run: {}\n", t.component, t.test, t.command));
        } else {
            out.push_str(&format!("  {} — `{}`\n    run: — ({})\n", t.component, t.test, t.note));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::manifest::{ComponentKind, Container, Layer};

    fn comp(name: &str, path: &str, tests: &[&str]) -> Component {
        Component {
            name: name.into(),
            kind: ComponentKind::Primitive,
            path: path.into(),
            summary: String::new(),
            tests: tests.iter().map(|s| s.to_string()).collect(),
            provides: vec![],
            children: vec![],
        }
    }

    fn manifest_with(components: Vec<Component>) -> Manifest {
        Manifest {
            schema_version: 1,
            layers: vec![Layer {
                name: "l".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                containers: vec![Container {
                    name: "c".into(),
                    path: String::new(),
                    description: String::new(),
                    sequence: vec![],
                    provides: vec![],
                    role: None,
                    gateway: None,
                    connects: vec![],
                    components,
                }],
            }],
        }
    }

    #[test]
    fn test_region_starts_at_first_marker() {
        let src = "pub fn a() {}\n// comment\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n";
        let r = test_region(src);
        assert!(r.starts_with("#[cfg(test)]"));
        assert!(r.contains("fn t()"));
        // No marker: empty region.
        assert_eq!(test_region("pub fn b() {}\n"), "");
        // Marker detection across attribute spellings.
        assert!(is_test_marker("    #[tokio::test]\n"));
        assert!(is_test_marker("#[rstest]\n"));
        assert!(!is_test_marker("fn test_marker_sounds_like_the_doc() {}\n"));
    }

    #[test]
    fn declared_but_missing_or_empty_test_paths_are_flagged() -> anyhow::Result<()> {
        let dir =
            std::env::temp_dir().join(format!("harna-declared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("real.rs"), "pub fn x() {}\n\n#[test]\nfn t() {}\n")?;
        std::fs::write(dir.join("stub.rs"), "pub fn no_tests_here() {}\n")?;

        let m = manifest_with(vec![
            comp("good", "real.rs", &["real.rs"]),
            comp("ghost", "real.rs", &["tests/never_written.rs"]),
            comp("stub", "stub.rs", &["stub.rs"]),
        ]);
        let mut v = Vec::new();
        check_declared(&dir, &m, &mut v);
        assert_eq!(v.len(), 2, "ghost path + empty stub; the well-tested one is clean");
        let err = v.iter().find(|x| x.severity == Severity::Error).unwrap();
        assert_eq!(err.check, "tests-declared");
        assert!(err.file.contains("never_written"));
        let info = v.iter().find(|x| x.severity == Severity::Info).unwrap();
        assert!(info.message.contains("no test code"));
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn changed_component_without_test_update_is_advised() {
        let m = manifest_with(vec![
            comp("covered", "src/covered.rs", &["src/covered.rs", "tests/covered_ext.rs"]),
            comp("other", "src/other.rs", &["src/other.rs"]),
        ]);
        let mut v = Vec::new();
        check_changes(
            std::env::temp_dir().as_path(),
            &m,
            &["src/covered.rs".into(), "src/unrelated.rs".into()],
            &mut v,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].check, "no-test-update");
        assert_eq!(v[0].file, "src/covered.rs");
        assert_eq!(v[0].severity, Severity::Info, "advice, not a gate");
        assert!(v[0].message.contains("covered_ext.rs"), "lists every registered test");
    }

    #[test]
    fn separate_test_file_touched_satisfies_the_update() {
        let m = manifest_with(vec![comp("covered", "src/covered.rs", &["src/covered.rs", "tests/covered_ext.rs"])]);
        let mut v = Vec::new();
        check_changes(
            std::env::temp_dir().as_path(),
            &m,
            &["src/covered.rs".into(), "tests/covered_ext.rs".into()],
            &mut v,
        );
        assert!(v.is_empty(), "a registered test changed alongside the component");
    }

    #[test]
    fn unregistered_components_owe_nothing() {
        // No `tests` declared: the check is silent (convention, not obligation).
        let m = manifest_with(vec![comp("free", "src/free.rs", &[])]);
        let mut v = Vec::new();
        check_changes(std::env::temp_dir().as_path(), &m, &["src/free.rs".into()], &mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn inline_update_detected_against_head_only_for_the_test_region() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!("harna-inline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        git(&root, &["init", "-q"]);
        // Committed: component with a real test section.
        std::fs::write(root.join("c.rs"), "pub fn keep() {}\n#[test]\nfn old() {}\n")?;
        git(&root, &["add", "."]);
        git(&root, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "c"]);

        let m = manifest_with(vec![comp("c", "c.rs", &["c.rs"])]);

        // Burst 1: only the shipped body changed; the test region is identical.
        std::fs::write(root.join("c.rs"), "pub fn moved_body() {}\n#[test]\nfn old() {}\n")?;
        let mut v = Vec::new();
        check_changes(&root, &m, &["c.rs".into()], &mut v);
        assert_eq!(v.len(), 1, "test untouched -> advice fires");

        // Burst 2: test region extended in the same burst.
        std::fs::write(root.join("c.rs"), "pub fn moved_body() {}\n#[test]\nfn old() {}\n#[test]\nfn added() {}\n")?;
        let mut v2 = Vec::new();
        check_changes(&root, &m, &["c.rs".into()], &mut v2);
        assert!(v2.is_empty(), "inline test update satisfies the obligation");

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn head_version_none_marks_untracked_files() {
        // A path outside any repository: head_version is None (not an error).
        let nowhere = std::env::temp_dir().join(format!("harna-nogit-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&nowhere);
        assert_eq!(head_version(&nowhere, "whatever.rs"), None);
    }

    #[test]
    fn coverage_ratio_floors_and_counts_nested() {
        let mut kids = vec![comp("kid-covered", "k.rs", &["k.rs"])];
        kids.push(comp("kid-bare", "b.rs", &[]));
        let mut top = comp("top", "t.rs", &["t.rs"]);
        top.children = kids;
        let m = manifest_with(vec![top]);
        let c = coverage(&m);
        assert_eq!((c.total, c.with_tests), (3, 2));
        assert_eq!(c.percent, 66, "floors, never flatters");
        assert_eq!(c.untested, vec!["kid-bare".to_string()], "sorted names");
    }

    #[test]
    fn coverage_empty_map_is_vacuously_full() {
        let c = coverage(&manifest_with(vec![]));
        assert_eq!((c.total, c.with_tests, c.percent), (0, 0, 100));
        assert!(c.untested.is_empty());
    }

    fn pkg_map() -> std::collections::BTreeMap<&'static str, &'static str> {
        [("crates/a", "pkg-a"), ("crates/b", "pkg-b"), (".", "root-pkg")]
            .into_iter()
            .collect()
    }

    #[test]
    fn inline_lib_files_map_to_module_filters() {
        let m = pkg_map();
        let t = map_one("c", "crates/a/src/x/y.rs", &m);
        assert!(t.runnable());
        assert_eq!(t.command, "cargo test -p pkg-a --lib x::y");
        assert!(t.note.contains("inline"));
        // `mod.rs` collapses to the parent module.
        let t = map_one("c", "crates/a/src/x/mod.rs", &m);
        assert_eq!(t.command, "cargo test -p pkg-a --lib x");
    }

    #[test]
    fn roots_and_binaries_run_the_whole_package() {
        let m = pkg_map();
        for p in ["crates/a/src/main.rs", "crates/a/src/lib.rs", "crates/a/src/bin/tool.rs"] {
            let t = map_one("c", p, &m);
            assert!(t.runnable(), "{p}");
            assert_eq!(t.command, "cargo test -p pkg-a");
        }
        // Root-crate src resolves through "." too.
        let t = map_one("c", "src/x.rs", &m);
        assert_eq!(t.command, "cargo test -p root-pkg --lib x");
    }

    #[test]
    fn integration_and_bench_targets_map_by_stem() {
        let m = pkg_map();
        let t = map_one("c", "crates/b/tests/cover_ext.rs", &m);
        assert_eq!(t.command, "cargo test -p pkg-b --test cover_ext");
        let t = map_one("c", "crates/b/benches/speed.rs", &m);
        assert_eq!(t.command, "cargo test -p pkg-b --bench speed");
        // Nested files are no cargo target: directories, not files, are targets.
        let t = map_one("c", "crates/b/tests/nested/extra.rs", &m);
        assert!(!t.runnable());
    }

    #[test]
    fn unmappable_paths_stay_visible_instead_of_guessing() {
        let m = pkg_map();
        let t = map_one("c", "docs/fixture.md", &m);
        assert!(!t.runnable() && t.command.is_empty());
        let t = map_one("c", "scripts/check.sh", &m);
        assert!(!t.runnable());
        let t = map_one("c", "crates/a/README.md", &m);
        assert!(!t.runnable(), "non-Rust asset");
        // Crate dir with no readable package: named, not guessed.
        let t = map_one("c", "crates/ghost/src/x.rs", &m);
        assert!(!t.runnable());
        assert!(t.note.contains("no readable package"));
        // Rust file outside src/tests/benches has no target shape.
        let t = map_one("c", "crates/a/build.rs", &m);
        assert!(!t.runnable());
    }

    #[test]
    fn runnable_resolves_packages_through_real_cargo_tomls() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "harna-testmap-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/a/src"))?;
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/a\"]\n")?;
        std::fs::write(root.join("crates/a/Cargo.toml"), "[package]\nname = \"real-a\"\nversion = \"0.1.0\"\n")?;
        let m = manifest_with(vec![
            comp("inline", "crates/a/src/deep/mod.rs", &["crates/a/src/deep/mod.rs"]),
            comp("it", "crates/a/tests/e2e.rs", &["crates/a/tests/e2e.rs"]),
        ]);
        let targets = runnable(&root, &m);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].command, "cargo test -p real-a --lib deep");
        assert_eq!(targets[1].command, "cargo test -p real-a --test e2e");
        let text = render_targets(&targets);
        assert!(text.contains("2 target(s), 2 runnable"));
        assert!(text.contains("cargo test -p real-a --lib deep"));
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    fn git(root: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git should be available for tests");
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
}
