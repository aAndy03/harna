use super::*;
use crate::schema::model::manifest::{ComponentKind, Container, Layer};

fn comp(name: &str, path: &str, tests: &[&str]) -> Component {
    Component {
        planned: false,
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
            role: None,
            containers: vec![Container {
                escapes: Vec::new(),
                owns: Vec::new(),
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
    let src =
        "pub fn a() {}\n// comment\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n";
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
    let dir = std::env::temp_dir().join(format!("harna-declared-{}", std::process::id()));
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
    assert_eq!(
        v.len(),
        2,
        "ghost path + empty stub; the well-tested one is clean"
    );
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
        comp(
            "covered",
            "src/covered.rs",
            &["src/covered.rs", "tests/covered_ext.rs"],
        ),
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
    assert!(
        v[0].message.contains("covered_ext.rs"),
        "lists every registered test"
    );
}

#[test]
fn separate_test_file_touched_satisfies_the_update() {
    let m = manifest_with(vec![comp(
        "covered",
        "src/covered.rs",
        &["src/covered.rs", "tests/covered_ext.rs"],
    )]);
    let mut v = Vec::new();
    check_changes(
        std::env::temp_dir().as_path(),
        &m,
        &["src/covered.rs".into(), "tests/covered_ext.rs".into()],
        &mut v,
    );
    assert!(
        v.is_empty(),
        "a registered test changed alongside the component"
    );
}

#[test]
fn unregistered_components_owe_nothing() {
    // No `tests` declared: the check is silent (convention, not obligation).
    let m = manifest_with(vec![comp("free", "src/free.rs", &[])]);
    let mut v = Vec::new();
    check_changes(
        std::env::temp_dir().as_path(),
        &m,
        &["src/free.rs".into()],
        &mut v,
    );
    assert!(v.is_empty());
}

#[test]
fn inline_update_detected_against_head_only_for_the_test_region() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("harna-inline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    git(&root, &["init", "-q"]);
    // Committed: component with a real test section.
    std::fs::write(
        root.join("c.rs"),
        "pub fn keep() {}\n#[test]\nfn old() {}\n",
    )?;
    git(&root, &["add", "."]);
    git(
        &root,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "c",
        ],
    );

    let m = manifest_with(vec![comp("c", "c.rs", &["c.rs"])]);

    // Burst 1: only the shipped body changed; the test region is identical.
    std::fs::write(
        root.join("c.rs"),
        "pub fn moved_body() {}\n#[test]\nfn old() {}\n",
    )?;
    let mut v = Vec::new();
    check_changes(&root, &m, &["c.rs".into()], &mut v);
    assert_eq!(v.len(), 1, "test untouched -> advice fires");

    // Burst 2: test region extended in the same burst.
    std::fs::write(
        root.join("c.rs"),
        "pub fn moved_body() {}\n#[test]\nfn old() {}\n#[test]\nfn added() {}\n",
    )?;
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
    [
        ("crates/a", "pkg-a"),
        ("crates/b", "pkg-b"),
        (".", "root-pkg"),
    ]
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
    for p in [
        "crates/a/src/main.rs",
        "crates/a/src/lib.rs",
        "crates/a/src/bin/tool.rs",
    ] {
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
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/a\"]\n",
    )?;
    std::fs::write(
        root.join("crates/a/Cargo.toml"),
        "[package]\nname = \"real-a\"\nversion = \"0.1.0\"\n",
    )?;
    let m = manifest_with(vec![
        comp(
            "inline",
            "crates/a/src/deep/mod.rs",
            &["crates/a/src/deep/mod.rs"],
        ),
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
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
