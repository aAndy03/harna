use super::*;
use crate::schema::model::rules::Rule;
use crate::search::digest::Module;

fn digest(paths: &[&str]) -> Digest {
    Digest {
        modules: paths
            .iter()
            .map(|p| Module {
                reexports: Vec::new(),
                crate_dir: None,
                src_root: None,
                path: p.to_string(),
                lang: "rust".into(),
                signatures: vec![],
                deps: vec![],
                body_hash: String::new(),
                inventory: vec![],
                mods: vec![],
            })
            .collect(),
        ..Default::default()
    }
}

fn rule(id: &str, check: &str, limit: u32, scope: &[&str]) -> Rule {
    Rule {
        scope_signature: None,
        command: None,
        values: Vec::new(),
        id: id.into(),
        text: "t".into(),
        rationale: "r".into(),
        scope: scope.iter().map(|s| s.to_string()).collect(),
        tags: vec![],
        origin: "test".into(),
        examples: vec![],
        env: None,
        check: Some(check.into()),
        severity: None,
        pattern: None,
        limit: Some(limit),
        status: None,
    }
}

#[test]
fn a_name_encoded_folder_is_named() {
    let d = digest(&[
        "src/schema/mod.rs",
        "src/schema/acknowledged_data.rs",
        "src/schema/deferred_data.rs",
        "src/schema/lessons_data.rs",
        "src/schema/manifest.rs",
        "src/schema/store.rs",
        "src/schema/store/tests.rs",
    ]);
    let mut v = Vec::new();
    check(&Rules::default(), None, Some(&d), &mut v);
    assert_eq!(v.len(), 1, "{v:#?}");
    assert_eq!(v[0].check, "name-cluster");
    assert!(
        v[0].message.contains("3 files share the name token `data`"),
        "{}",
        v[0].message
    );
    assert!(v[0].hint.contains("src/schema/data/"));
    // Already grouped: a folder named after the token is the fix, not a finding.
    let grouped = digest(&[
        "src/data/a_data.rs",
        "src/data/b_data.rs",
        "src/data/c_data.rs",
    ]);
    let mut v = Vec::new();
    check(&Rules::default(), None, Some(&grouped), &mut v);
    assert!(v.is_empty(), "{v:#?}");
}

#[test]
fn a_bare_stem_counts_toward_its_own_cluster() {
    let c = clusters(&[
        "parity".into(),
        "parity_policy".into(),
        "parity_views".into(),
        "store".into(),
    ]);
    assert_eq!(c["parity"].len(), 3);
    assert!(!c.contains_key("store"), "a lone stem is not a cluster");
}

#[test]
fn folder_size_needs_a_rule_and_counts_members_only() {
    let d = digest(&[
        "src/a/mod.rs",
        "src/a/tests.rs",
        "src/a/x.rs",
        "src/a/y.rs",
        "src/a/z.rs",
    ]);
    let mut v = Vec::new();
    check(&Rules::default(), None, Some(&d), &mut v);
    assert!(
        v.iter().all(|v| v.check != "folder-size"),
        "no rule, no budget"
    );

    let rules = Rules {
        schema_version: 1,
        rules: vec![rule("fb", "folder-size", 2, &["src"])],
    };
    let mut v = Vec::new();
    check(&rules, None, Some(&d), &mut v);
    let hit = v
        .iter()
        .find(|v| v.check == "folder-size")
        .expect("over budget");
    assert!(
        hit.message.contains("holds 3 source files (budget 2)"),
        "mod.rs and tests.rs not counted: {}",
        hit.message
    );
    // Out of the rule's scope: silent.
    let rules = Rules {
        schema_version: 1,
        rules: vec![rule("fb", "folder-size", 2, &["other"])],
    };
    let mut v = Vec::new();
    check(&rules, None, Some(&d), &mut v);
    assert!(v.iter().all(|v| v.check != "folder-size"));
}

#[test]
fn a_rule_can_tighten_the_cluster_threshold() {
    let d = digest(&["src/hook_setup.rs", "src/hook_events.rs"]);
    let mut v = Vec::new();
    check(&Rules::default(), None, Some(&d), &mut v);
    assert!(v.is_empty(), "two is not yet a cluster by default");
    let rules = Rules {
        schema_version: 1,
        rules: vec![rule("nc", "name-cluster", 2, &[])],
    };
    let mut v = Vec::new();
    check(&rules, None, Some(&d), &mut v);
    assert!(v.iter().any(|v| v.message.contains("`hook`")), "{v:#?}");
}
