use super::*;

#[test]
fn run_on_scopes_file_checks_to_listed_files() {
    // A standalone file check through run_on's per-file path.
    let root = std::env::temp_dir().join(format!("harna-runon-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".harna")).unwrap();
    std::fs::write(root.join(".harna/rules.toml"), "schema_version = 1\n").unwrap();
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::write(
        root.join("src/api/handlers.rs"),
        "fn bad() { fetch(\"https://api.example.com\"); }",
    )
    .unwrap();
    std::fs::write(root.join("src/api/clean.rs"), "pub fn good() {}").unwrap();

    let store = crate::schema::store::Store::open(&root).unwrap();
    // Only the clean file is checked: no violations.
    let scoped = run_on(
        &root,
        &store,
        100,
        Some(vec!["src/api/clean.rs".into()]),
        None,
    )
    .unwrap();
    assert!(scoped.is_empty());
    // Full scan finds the violation.
    let full = run(&root, &store, 100).unwrap();
    assert!(full.iter().any(|v| v.check == "no-hardcoded-urls"));
}

#[test]
fn pattern_rules_run_env_scoped_through_run_on() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("harna-pattern-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".harna")).unwrap();
    std::fs::write(
        root.join(".harna/rules.toml"),
        "schema_version = 1\n\n[[rules]]\nid = \"dev-naming\"\ntext = \"dev handlers keep the mockstripe adapter\"\nrationale = \"No real charges from dev sessions\"\norigin = \"test\"\nenv = \"dev\"\nseverity = \"error\"\npattern = \"(?i)mock_stripe\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::write(
        root.join("src/api/payment.rs"),
        "pub struct Mock_Stripe;\npub fn good() {}",
    )
    .unwrap();

    let store = crate::schema::store::Store::open(&root).unwrap();
    // No env declared: the dev rule is out of enforcement.
    let none = run_on(&root, &store, 100, None, None).unwrap();
    assert!(
        none.iter().all(|v| v.check != "pattern"),
        "env-less run ignores dev rules"
    );
    // Declared prod: still out.
    let prod = run_on(&root, &store, 100, None, Some("prod")).unwrap();
    assert!(prod.iter().all(|v| v.check != "pattern"));
    // Declared dev: the pattern fires on the declared name, as an Error.
    let dev = run_on(&root, &store, 100, None, Some("dev")).unwrap();
    let hit = dev
        .iter()
        .find(|v| v.check == "pattern")
        .expect("dev run must see the pattern rule");
    assert_eq!(hit.severity, Severity::Error, "the rule's override wins");
    assert_eq!(hit.file, "src/api/payment.rs");
    assert!(hit.message.contains("Mock_Stripe"));
    assert!(
        hit.message.contains("declared name"),
        "names are policeable too"
    );
    Ok(())
}

#[test]
fn bad_pattern_regex_is_a_visible_violation_through_run_on() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("harna-badpat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".harna")).unwrap();
    std::fs::write(
        root.join(".harna/rules.toml"),
        "schema_version = 1\n\n[[rules]]\nid = \"broken\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\npattern = \"(unclosed\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/x.rs"), "pub fn y() {}").unwrap();

    let store = crate::schema::store::Store::open(&root).unwrap();
    let v = run_on(&root, &store, 100, None, None).unwrap();
    assert!(v
        .iter()
        .any(|viol| viol.message.contains("un-compilable pattern")));
    Ok(())
}

#[test]
fn accretion_is_advisory_never_gating() {
    let big = "pub fn x() {}\n".repeat(700);
    let mut v = Vec::new();
    check_accretion("src/big.rs", &big, ACCRETION_LINES, &mut v);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].severity, Severity::Info, "guidance, not a limit");
    assert!(v[0].message.contains("no limit"));
    assert!(
        v[0].message.contains(&format!("past {ACCRETION_LINES}")),
        "the trigger number rides the message: {}",
        v[0].message
    );
    assert_eq!(v[0].line, 0);
    // small file: nothing
    let mut v2 = Vec::new();
    check_accretion("src/small.rs", "pub fn x() {}", ACCRETION_LINES, &mut v2);
    assert!(v2.is_empty());
    // 0.3.43: a bound limit moves the advisory and names its source.
    let mut v3 = Vec::new();
    check_accretion("src/big.rs", &big, 800, &mut v3);
    assert!(v3.is_empty(), "700 lines under a bound 800");
    check_accretion("src/big.rs", &big, 300, &mut v3);
    assert!(
        v3[0].message.contains("past 300, rules.toml"),
        "{}",
        v3[0].message
    );
}

fn data_fixture(tag: &str) -> (std::path::PathBuf, crate::schema::store::Store) {
    let root = std::env::temp_dir().join(format!("harna-data-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let store = crate::schema::store::Store::open(&root).unwrap();
    store.init().unwrap();
    (root, store)
}

#[test]
fn unreadable_manifest_or_rules_fail_closed() {
    for (tag, file) in [("manifest", "manifest.toml"), ("rules", "rules.toml")] {
        let (root, store) = data_fixture(tag);
        std::fs::write(root.join(".harna").join(file), "schema_version = 1\n[[x]\n").unwrap();
        let v = run_on(&root, &store, 100, Some(vec![]), None).unwrap();
        let hit = v
            .iter()
            .find(|v| v.check == "harna-data" && v.file.ends_with(file))
            .unwrap_or_else(|| panic!("broken {file} must surface: {v:?}"));
        assert_eq!(hit.severity, Severity::Error);
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn newer_schema_manifest_fails_closed() {
    let (root, store) = data_fixture("newer");
    std::fs::write(store.manifest_path(), "schema_version = 999\n").unwrap();
    let v = run_on(&root, &store, 100, Some(vec![]), None).unwrap();
    assert!(
        v.iter()
            .any(|v| v.check == "harna-data" && v.severity == Severity::Error),
        "{v:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unparseable_parity_event_fails_closed() {
    let (root, store) = data_fixture("parity");
    let dir = root.join(".harna/parity");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("conflicted.toml"), "<<<<<<< HEAD\nid = \"x\"\n").unwrap();
    let v = run_on(&root, &store, 100, Some(vec![]), None).unwrap();
    assert!(
        v.iter()
            .any(|v| v.check == "harna-data" && v.file == ".harna/parity/"),
        "a merge-conflicted event must not hide every open debt: {v:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_test_file_moved_out_of_line_keeps_its_exemptions() {
    let (root, store) = data_fixture("testfile");
    std::fs::create_dir_all(root.join("src/a")).unwrap();
    let fixture = "fn t() { let _ = \"https://api.example.com\"; }\n";
    std::fs::write(root.join("src/a/tests.rs"), fixture).unwrap();
    std::fs::write(root.join("src/a/real.rs"), fixture).unwrap();
    let v = run_on(
        &root,
        &store,
        100,
        Some(vec!["src/a/tests.rs".into(), "src/a/real.rs".into()]),
        None,
    )
    .unwrap();
    assert!(
        v.iter()
            .any(|v| v.check == "no-hardcoded-urls" && v.file == "src/a/real.rs"),
        "{v:?}"
    );
    assert!(
        !v.iter().any(|v| v.file == "src/a/tests.rs"),
        "test fixtures are not shipped code: {v:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_healthy_fresh_repo_has_no_data_errors() {
    let (root, store) = data_fixture("healthy");
    let v = run_on(&root, &store, 100, Some(vec![]), None).unwrap();
    assert!(v.iter().all(|v| v.check != "harna-data"), "{v:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// deno (0.3.44): 174 `no-hardcoded-urls` errors on day one, from a check
/// no rule asked for. Unbound it advises; a rule named after it (or
/// binding it) makes it gate.
#[test]
fn unadopted_content_checks_advise_until_a_rule_binds_them() {
    let (root, store) = data_fixture("adopt-urls");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/fetch.rs"),
        "pub fn base() -> &'static str { \"https://registry.example.com\" }\n",
    )
    .unwrap();
    let files = Some(vec!["src/fetch.rs".to_string()]);
    let url = |v: &[Violation]| {
        v.iter()
            .find(|x| x.check == "no-hardcoded-urls")
            .cloned()
            .expect("the URL is still named")
    };
    let v = run_on(&root, &store, 100, files.clone(), None).unwrap();
    let hit = url(&v);
    assert_eq!(hit.severity, Severity::Info, "unbound: advisory");
    assert!(
        hit.hint.contains("advisory until a rule binds it"),
        "{}",
        hit.hint
    );

    let mut rules = store.load_rules().unwrap();
    rules.rules.push(crate::schema::model::rules::Rule {
        id: "no-hardcoded-urls".into(),
        text: "No URL literals".into(),
        rationale: "swappable per environment".into(),
        origin: "test".into(),
        ..Default::default()
    });
    store.save_rules(&rules).unwrap();
    let v = run_on(&root, &store, 100, files, None).unwrap();
    assert_eq!(url(&v).severity, Severity::Error, "adopted by its rule");
    let _ = std::fs::remove_dir_all(&root);
}
