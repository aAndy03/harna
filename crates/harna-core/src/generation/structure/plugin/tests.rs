use super::*;

fn store(tag: &str) -> (std::path::PathBuf, Store) {
    let root = std::env::temp_dir().join(format!(
        "harna-plugin-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let store = Store::open(&root).unwrap();
    store.init().unwrap();
    (root, store)
}

/// 0.5.0: the shipped gpui-kit pack loads, installs into .harna/plugins,
/// its rules join the project's namespaced, and removal takes them out.
#[test]
fn a_plugin_is_vendored_and_its_rules_join_namespaced() {
    let (root, store) = store("add");
    let pack = resolve("gpui-kit", None).unwrap();
    assert_eq!(
        pack.meta.mirrors.get("gpui-kit").map(String::as_str),
        Some("0.8.0")
    );
    assert!(!pack.rules().rules.is_empty());
    let lines = add(&store, &pack).unwrap();
    assert!(lines[0].contains("added"), "{lines:?}");
    let rules = store.load_effective_rules().unwrap();
    assert!(rules
        .rules
        .iter()
        .any(|r| r.id == "gpui-kit/stable-identity" && r.origin.starts_with("plugin gpui-kit")));
    assert!(
        store.load_rules().unwrap().rules.is_empty(),
        "rules.toml untouched"
    );
    let terms = store.load_effective_terms().unwrap();
    assert!(terms.terms.iter().any(|t| t.word == "entity"));
    assert!(add(&store, &pack).unwrap()[0].contains("re-added"));
    remove(&store, "gpui-kit").unwrap();
    assert!(store.load_effective_rules().unwrap().rules.is_empty());
    assert!(remove(&store, "gpui-kit").is_err());
    assert!(remove(&store, "../x").is_err());
    let _ = std::fs::remove_dir_all(&root);
}

/// The doctor's drift reading: a lock that moved to another 0.x line than
/// the plugin mirrors is named; a patch release is not.
#[test]
fn drift_compares_the_release_line() {
    let (root, store) = store("drift");
    add(&store, &resolve("gpui-kit", None).unwrap()).unwrap();
    let lock = |v: &str| {
        std::fs::write(
            root.join("Cargo.lock"),
            format!("version = 3\n\n[[package]]\nname = \"gpui-kit\"\nversion = \"{v}\"\n"),
        )
        .unwrap()
    };
    lock("0.8.3");
    assert!(drift(&root, &store).is_empty());
    lock("0.9.0");
    let d = drift(&root, &store);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].3, "0.9.0");
    let _ = std::fs::remove_dir_all(&root);
}

/// 0.5.9 review H1/M3/M4/M1: names are plain words (no `C:` escape), a
/// folder must carry its own name, a broken pack is skipped (the project's
/// rules and the healthy packs still apply), and a plugin rule is switched
/// off through a project override that survives re-adding the pack.
#[test]
fn plugin_names_folders_breakage_and_overrides() {
    let (root, store) = store("guard");
    assert!(!crate::schema::store::plugins::valid_plugin_name("C:"));
    assert!(!crate::schema::store::plugins::valid_plugin_name("../x"));
    assert!(crate::schema::store::plugins::valid_plugin_name("gpui-kit"));
    assert!(remove(&store, "C:").is_err());
    let evil = root.join("evil");
    std::fs::create_dir_all(&evil).unwrap();
    std::fs::write(evil.join("plugin.toml"), "name = \"C:\"\n").unwrap();
    assert!(from_dir(&evil).is_err());

    add(&store, &resolve("gpui-kit", None).unwrap()).unwrap();
    // a copy under another folder name is refused at load, not doubled
    let copy = store.plugins_dir().join("gk-old");
    std::fs::create_dir_all(&copy).unwrap();
    std::fs::copy(
        store.plugins_dir().join("gpui-kit/plugin.toml"),
        copy.join("plugin.toml"),
    )
    .unwrap();
    let (ok, broken) = store.load_plugins_lenient();
    assert_eq!(ok.len(), 1);
    assert_eq!(broken.len(), 1);
    let ids: Vec<String> = store
        .load_effective_rules()
        .unwrap()
        .rules
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(
        ids.iter()
            .filter(|i| *i == "gpui-kit/stable-identity")
            .count(),
        1
    );
    std::fs::remove_dir_all(&copy).unwrap();

    // deprecate through an override; re-adding the pack keeps it
    let mut rules = store.load_rules().unwrap();
    assert!(store.adopt_plugin_rule(&mut rules, "gpui-kit/stable-identity"));
    rules
        .deprecate("gpui-kit/stable-identity", "not here")
        .unwrap();
    store.save_rules(&rules).unwrap();
    add(&store, &resolve("gpui-kit", None).unwrap()).unwrap();
    let eff = store.load_effective_rules().unwrap();
    let r: Vec<_> = eff
        .rules
        .iter()
        .filter(|r| r.id == "gpui-kit/stable-identity")
        .collect();
    assert_eq!(r.len(), 1);
    assert!(r[0].is_deprecated());
    let _ = std::fs::remove_dir_all(&root);
}
