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

    fn git(root: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git should be available for tests");
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
}
