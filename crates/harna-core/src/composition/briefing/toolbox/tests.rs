use super::*;
use crate::schema::model::manifest::{Component, Layer};

/// The catalog is data the code must keep honest: every tool names a real
/// check, every signal is one this harna implements, ids are unique.
#[test]
fn the_catalog_is_coherent_with_the_checks_and_signals() {
    let known_signals = [
        "external-crate-single-user",
        "imported-across-layers",
        "no-rules-in-scope",
        "untested-components",
        "imported-without-surface",
        "no-qualities-declared",
    ];
    let mut ids = std::collections::BTreeSet::new();
    for t in catalog() {
        assert!(ids.insert(t.id.clone()), "duplicate tool id {}", t.id);
        if let Some(c) = &t.enforces {
            assert!(
                crate::enforcement::checks::known(c),
                "{}: unknown check {c}",
                t.id
            );
        }
        if let Some(s) = &t.signal {
            assert!(
                known_signals.contains(&s.as_str()),
                "{}: unknown signal {s}",
                t.id
            );
        }
    }
    let text = render_catalog();
    assert!(text.contains("Crate gateway") && text.contains("how: harna"));
}

/// ledger field test (0.3.50): the agent never reached for a crate gateway,
/// registered tests or a scoped rule until told. `place` offers them where
/// the structure says they fit.
#[test]
fn tools_are_offered_where_the_structure_fits() {
    let root = std::env::temp_dir().join(format!(
        "harna-toolbox-{}-{:?}",
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
        "crates/storage/Cargo.toml",
        "[package]\nname = \"storage\"\n[dependencies]\nrusqlite = \"0.32\"\nserde = \"1\"\n",
    );
    w("crates/storage/src/lib.rs", "");
    w("crates/storage/tests/repos.rs", "");
    w(
        "crates/domain/Cargo.toml",
        "[package]\nname = \"domain\"\n[dependencies]\nserde = \"1\"\n",
    );
    w("crates/domain/src/lib.rs", "");
    let manifest = Manifest {
        layers: vec![Layer {
            name: "storage".into(),
            containers: vec![Container {
                name: "sqlite".into(),
                path: "crates/storage/src".into(),
                components: vec![Component {
                    name: "repos".into(),
                    path: "crates/storage/src/lib.rs".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let block = suggestions(&root, &manifest, &Rules::default(), None, &["sqlite"]);
    assert!(
        block.contains("only `storage` depends on `rusqlite`"),
        "{block}"
    );
    assert!(!block.contains("`serde`"), "two crates use serde: {block}");
    assert!(block.contains("no registered tests"), "{block}");
    assert!(block.contains("no rule covers `sqlite`"), "{block}");
    assert!(
        block.contains("optional"),
        "suggestions, never requirements"
    );
    // Owned already: the crate gateway is no longer offered.
    let mut owned = manifest.clone();
    owned.layers[0].containers[0].gateway = Some("crate:rusqlite".into());
    let block = suggestions(&root, &owned, &Rules::default(), None, &["sqlite"]);
    assert!(!block.contains("rusqlite"), "{block}");
    let _ = std::fs::remove_dir_all(&root);
}
