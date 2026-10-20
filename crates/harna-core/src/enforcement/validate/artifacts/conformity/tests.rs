use super::*;
use crate::schema::model::rules::Rule;

/// codex (0.3.48, R4): every crate has a `BUILD.bazel`; zed's
/// `package_conformity.rs`: every Cargo.toml says `lints.workspace = true`.
/// One rule, each crate named with what it lacks.
#[test]
fn every_crate_in_scope_carries_its_companions_and_keys() {
    let root = std::env::temp_dir().join(format!(
        "harna-conformity-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\", \"tools/x\"]\n",
    );
    w(
        "crates/good/Cargo.toml",
        "[package]\nname = \"good\"\n[lints]\nworkspace = true\n",
    );
    w("crates/good/BUILD.bazel", "codex_rust_crate()\n");
    w(
        "crates/bad/Cargo.toml",
        "[package]\nname = \"bad\"\n[lints]\nworkspace = false\n",
    );
    w("tools/x/Cargo.toml", "[package]\nname = \"x\"\n");
    let mut rules = Rules::default();
    rules.rules.push(Rule {
        id: "crate-shape".into(),
        rationale: "Bazel mirrors Cargo; lints are workspace-wide".into(),
        check: Some("crate-conformity".into()),
        scope: vec!["crates".into()],
        values: vec![
            "file:BUILD.bazel".into(),
            "toml:lints.workspace = true".into(),
        ],
        ..Default::default()
    });
    let mut out = Vec::new();
    check(&root, &rules, None, &mut out);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].file, "crates/bad/Cargo.toml");
    assert!(
        out[0]
            .message
            .contains("crate `bad` lacks file:BUILD.bazel, toml:lints.workspace = true"),
        "{}",
        out[0].message
    );
    // A presence-only key.
    rules.rules[0].values = vec!["toml:lints.workspace".into()];
    let mut out = Vec::new();
    check(&root, &rules, None, &mut out);
    assert!(out.is_empty(), "{out:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// 0.4.2 (gpui-kit profile): `!` inverts a requirement — the headless
/// crates must NOT depend on gpui-kit.
#[test]
fn a_negated_requirement_must_not_hold() {
    let root = std::env::temp_dir().join(format!(
        "harna-conformity-neg-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    w(
        "crates/shared/Cargo.toml",
        "[package]\nname = \"shared\"\n[dependencies]\ngpui-kit = \"0.8\"\n",
    );
    w(
        "crates/platform/Cargo.toml",
        "[package]\nname = \"platform\"\n",
    );
    let mut rules = Rules::default();
    rules.rules.push(Rule {
        id: "headless".into(),
        check: Some("crate-conformity".into()),
        scope: vec!["crates/shared".into(), "crates/platform".into()],
        values: vec!["!toml:dependencies.gpui-kit".into()],
        ..Default::default()
    });
    let mut out = Vec::new();
    check(&root, &rules, None, &mut out);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].file, "crates/shared/Cargo.toml");
    let _ = std::fs::remove_dir_all(&root);
}
