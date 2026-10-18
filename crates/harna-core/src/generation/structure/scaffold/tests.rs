use super::*;
use std::path::PathBuf;

fn tmp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "harna-scaffold-{}-{}",
        std::process::id(),
        name_of_test()
    ));
    // Fresh dir every run: pid reuse across runs would otherwise resurrect
    // stale manifests (scaffold refuses to overwrite existing files).
    let _ = std::fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn temp() -> PathBuf {
    tmp_dir()
}

fn name_of_test() -> String {
    // The test thread's name IS the test id under cargo — a unique,
    // collision-free tag (the wall-clock nanos it replaced were not
    // guaranteed distinct on Windows, so same-tick tests shared a temp
    // repo and infected each other's manifests). Rust separators do not
    // belong in paths on Windows — sanitize them.
    std::thread::current()
        .name()
        .unwrap_or("scaffold")
        .replace("::", "__")
}

#[test]
fn primitive_scaffold_creates_file_and_registers() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    let req = ScaffoldRequest {
        layer: "data-access".into(),
        container: "users".into(),
        name: "user-repo".into(),
        kind: ScaffoldKind::Primitive,
        path: None,
        description: "persists users".into(),
        role: None,
    };
    let res = run(&root, &store, &req).unwrap();
    assert_eq!(res.components, vec!["user-repo"]);
    let file = root.join("src/data_access/users/user_repo.rs");
    assert!(file.exists(), "file created");

    let m = store.load_manifest().unwrap();
    assert_eq!(m.layers.len(), 1);
    assert_eq!(m.layers[0].containers[0].components[0].name, "user-repo");
    assert_eq!(
        m.layers[0].containers[0].components[0].kind,
        ComponentKind::Primitive
    );
    // The request description is the component's rendered summary — the
    // view prose must exist as data, not vanish in the manifest rewrite.
    assert_eq!(
        m.layers[0].containers[0].components[0].summary,
        "persists users"
    );
    // Born with its test, and registered with it.
    assert_eq!(
        m.layers[0].containers[0].components[0].tests,
        vec!["src/data_access/users/user_repo.rs".to_string()]
    );
    // No crate root in this fixture: scaffold says so instead of inventing one.
    assert!(
        res.message
            .contains("not wired into the module tree: no crate root"),
        "{}",
        res.message
    );
}

#[test]
fn scaffold_in_a_crate_compiles_into_the_module_tree() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "//! Demo crate.\n").unwrap();
    let req = ScaffoldRequest {
        layer: "engine".into(),
        container: "billing".into(),
        name: "invoice-total".into(),
        kind: ScaffoldKind::Primitive,
        path: None,
        description: "sums invoice lines".into(),
        role: None,
    };
    let res = run(&root, &store, &req).unwrap();
    assert!(root.join("src/engine/billing/invoice_total.rs").exists());
    let read = |p: &str| std::fs::read_to_string(root.join(p)).unwrap();
    assert!(read("src/lib.rs").contains("pub mod engine;"));
    assert!(read("src/engine/mod.rs").contains("pub mod billing;"));
    assert!(read("src/engine/billing/mod.rs").contains("pub mod invoice_total;"));
    assert!(res.message.contains("wired: "), "{}", res.message);
    assert!(
        res.message.contains("created src/engine/billing/mod.rs"),
        "{}",
        res.message
    );
}

#[test]
fn paths_escaping_the_repo_are_refused_before_any_write() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    store.init().unwrap();
    let manifest_before = std::fs::read_to_string(store.manifest_path()).unwrap();
    let req = |path: Option<&str>, container: &str, name: &str| ScaffoldRequest {
        layer: "engine".into(),
        container: container.into(),
        name: name.into(),
        kind: ScaffoldKind::Primitive,
        path: path.map(Into::into),
        description: "d".into(),
        role: None,
    };
    for bad in [
        req(Some("../../escaped"), "c", "n"),
        req(Some("/abs/escaped"), "c", "n"),
        req(Some("C:/abs/escaped"), "c", "n"),
        req(None, "../..", "n"),
        req(None, "c", "../../escaped"),
    ] {
        assert!(
            run(&root, &store, &bad).is_err(),
            "{:?} / {} / {}",
            bad.path,
            bad.container,
            bad.name
        );
    }
    assert!(scaffold_transiter(&root, &store, "a", "b", Some("../../../t".into())).is_err());
    assert_eq!(
        std::fs::read_to_string(store.manifest_path()).unwrap(),
        manifest_before
    );
    assert!(!root.parent().unwrap().join("escaped.rs").exists());
    // Plain nested relative paths stay allowed.
    assert!(run(&root, &store, &req(Some("src/ok/inner"), "c", "fine")).is_ok());
}

#[test]
fn broken_manifest_is_refused_not_overwritten() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    store.init().unwrap();
    let broken = "schema_version = 1\n[[layers]\nname = \"half-edited\"\n";
    std::fs::write(store.manifest_path(), broken).unwrap();
    let req = ScaffoldRequest {
        layer: "engine".into(),
        container: "billing".into(),
        name: "ledger".into(),
        kind: ScaffoldKind::Primitive,
        path: None,
        description: "d".into(),
        role: None,
    };
    assert!(run(&root, &store, &req).is_err());
    assert_eq!(
        std::fs::read_to_string(store.manifest_path()).unwrap(),
        broken,
        "the user's manifest keeps its bytes"
    );
    assert!(!root.join("src/engine/billing/ledger.rs").exists());
}

#[test]
fn compound_scaffold_creates_folder_with_child() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    let req = ScaffoldRequest {
        layer: "engine".into(),
        container: "billing".into(),
        name: "billing-engine".into(),
        kind: ScaffoldKind::Compound,
        path: None,
        description: String::new(),
        role: None,
    };
    let res = run(&root, &store, &req).unwrap();
    assert_eq!(res.components.len(), 2); // compound + starter child
    assert!(root
        .join("src/engine/billing/billing_engine/mod.rs")
        .exists());
    assert!(root
        .join("src/engine/billing/billing_engine/billing_engine_core.rs")
        .exists());

    let m = store.load_manifest().unwrap();
    let comp = &m.layers[0].containers[0].components[0];
    assert_eq!(comp.kind, ComponentKind::Compound);
    assert_eq!(comp.children.len(), 1);
    assert_eq!(comp.children[0].name, "billing-engine-core");
}

#[test]
fn a_declared_role_stamps_the_container_and_is_validated() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    let req = ScaffoldRequest {
        layer: "engines".into(),
        container: "billing".into(),
        name: "billing-motor".into(),
        kind: ScaffoldKind::Primitive,
        path: None,
        description: "sync billing".into(),
        role: Some("engine".into()),
    };
    let res = run(&root, &store, &req).unwrap();
    assert!(
        res.message.contains("container role: engine"),
        "the stamp is read aloud"
    );
    let m = store.load_manifest().unwrap();
    assert_eq!(
        m.layers[0].containers[0].role.as_deref(),
        Some("engine"),
        "the request's role is manifest data"
    );
    // An unknown role is refused with the vocabulary named (data, not
    // invented).
    let bad = ScaffoldRequest {
        role: Some("library".into()),
        ..req.clone()
    };
    let err = run(&root, &store, &bad).unwrap_err().to_string();
    assert!(err.contains("unknown role `library`"), "was: {err}");
    assert!(
        err.contains("orchestration-async"),
        "the vocabulary is in the hint"
    );
}

#[test]
fn duplicate_component_name_is_rejected() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    let req = |path: Option<String>| ScaffoldRequest {
        layer: "data-access".into(),
        container: "users".into(),
        name: "user-repo".into(),
        kind: ScaffoldKind::Primitive,
        path: path.clone(),
        description: String::new(),
        role: None,
    };
    run(&root, &store, &req(None)).unwrap();
    assert!(run(&root, &store, &req(None)).is_err());
}

#[test]
fn data_scaffold_creates_source_and_loader_pair() {
    let tmp = temp();
    let store = Store::open(&tmp).unwrap();
    let req = ScaffoldRequest {
        layer: "api".into(),
        container: "timezones".into(),
        name: "timezones".into(),
        kind: ScaffoldKind::Data,
        path: None,
        description: "every timezone the calendar supports".into(),
        role: None,
    };
    let res = run(&tmp, &store, &req).unwrap();
    assert_eq!(res.components, vec!["timezones", "timezones-loader"]);

    let toml_text = std::fs::read_to_string(tmp.join("src/api/timezones/timezones.toml")).unwrap();
    assert!(toml_text.contains("[[entries]]"));
    let loader =
        std::fs::read_to_string(tmp.join("src/api/timezones/timezones_loader.rs")).unwrap();
    assert!(loader.contains("include_str!"));
    assert!(
        loader.contains("constants-are-data") == false,
        "principle lives in rules, not code"
    );

    // Manifest registers both, loader owned by the data component.
    let m = store.load_manifest().unwrap();
    let comp = &m.layers[0].containers[0].components[0];
    assert_eq!(comp.name, "timezones");
    assert_eq!(comp.children[0].name, "timezones-loader");
    // Boot order includes both.
    assert!(m.layers[0].containers[0]
        .sequence
        .contains(&"timezones-loader".into()));
}

#[test]
fn transiter_goes_to_transit_layer() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    let res = scaffold_transiter(&root, &store, "api", "billing", None).unwrap();
    assert!(res.components[0].starts_with("transiter-api-to-billing"));
    let m = store.load_manifest().unwrap();
    assert_eq!(m.layers[0].name, "transit");
    // The generated container declares the corridor it bridges.
    let corridor = &m.layers[0].containers[0];
    assert_eq!(
        corridor.connects,
        vec!["api".to_string(), "billing".to_string()]
    );
}

#[test]
fn transiter_connects_stamp_never_clobbers_hand_edits() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    // Scaffold once, then hand-edit `connects` to reflect a deliberate
    // third leg; a second scaffold over the same container keeps it.
    scaffold_transiter(&root, &store, "api", "billing", None).unwrap();
    let mut m = store.load_manifest().unwrap();
    m.layers[0].containers[0].connects = vec!["api".into(), "billing".into(), "auth".into()];
    store.save_manifest(&m).unwrap();
    scaffold_transiter(
        &root,
        &store,
        "api",
        "billing",
        Some("transiter-again".into()),
    )
    .unwrap();
    let m = store.load_manifest().unwrap();
    let corridor = &m.layers[0].containers[0];
    assert_eq!(corridor.connects.len(), 3, "hand-edited corridor stays");
    assert_eq!(
        corridor.connects[2],
        "auth".to_string(),
        "the hand-written entry survives the scaffold"
    );
    // And the second scaffolded component still registered itself.
    assert!(corridor
        .components
        .iter()
        .any(|c| c.name == "transiter-again"));
}
