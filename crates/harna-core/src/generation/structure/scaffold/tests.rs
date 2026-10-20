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
    // No stub test counts as coverage (0.3.50): real tests are registered
    // when written.
    assert!(m.layers[0].containers[0].components[0].tests.is_empty());
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
        !loader.contains("constants-are-data"),
        "principle lives in rules, not code"
    );

    // Manifest registers both, loader owned by the data component.
    let m = store.load_manifest().unwrap();
    let comp = &m.layers[0].containers[0].components[0];
    assert_eq!(comp.name, "timezones");
    assert_eq!(comp.children[0].name, "timezones-loader");
    // Boot order is opt-in (0.3.50): none declared, none invented.
    assert!(m.layers[0].containers[0].sequence.is_empty());
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

/// font-manager (0.3.42): a binary-only crate scaffolded its backend into
/// `src/main.rs` silently, every container was born with `path = ""` (so
/// path-governed checks saw nothing), and a second `error` component in
/// another container failed with no hint that names are global.
#[test]
fn containers_are_born_with_paths_and_wiring_names_the_crate_root() {
    let root = tmp_dir();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    let store = Store::open(&root).unwrap();
    let req = |container: &str, name: &str, path: Option<&str>| ScaffoldRequest {
        layer: "backend".into(),
        container: container.into(),
        name: name.into(),
        kind: ScaffoldKind::Primitive,
        path: path.map(str::to_string),
        description: String::new(),
        role: None,
    };
    let res = run(&root, &store, &req("discovery", "scanner", None)).unwrap();
    assert!(
        res.message
            .contains("container discovery path: src/backend/discovery"),
        "{}",
        res.message
    );
    assert!(
        res.message.contains("crate root is the binary src/main.rs"),
        "{}",
        res.message
    );
    let res = run(
        &root,
        &store,
        &req(
            "collections",
            "store",
            Some("src/backend/collections/store.rs"),
        ),
    )
    .unwrap();
    assert!(
        res.message.contains("path: src/backend/collections"),
        "{}",
        res.message
    );

    let m = store.load_manifest().unwrap();
    let paths: Vec<&str> = m.layers[0]
        .containers
        .iter()
        .map(|c| c.path.as_str())
        .collect();
    assert_eq!(
        paths,
        vec!["src/backend/discovery", "src/backend/collections"]
    );

    // A library root, once it exists, is preferred and not flagged.
    fs::write(root.join("src/lib.rs"), "pub mod backend;\n").unwrap();
    let res = run(&root, &store, &req("discovery", "walker", None)).unwrap();
    assert!(!res.message.contains("binary"), "{}", res.message);
    assert!(
        !res.message.contains("path:"),
        "path already set: {}",
        res.message
    );

    let err = run(&root, &store, &req("collections", "scanner", None)).unwrap_err();
    assert!(
        err.to_string().contains("unique across the whole manifest"),
        "{err}"
    );
    assert!(err.to_string().contains("`collections-scanner`"), "{err}");
}

/// ledger field test (0.3.49): container `budget` + component `budget` was
/// born `budget/budget.rs` — clippy's module_inception. Refused up front;
/// an explicit --path names its real home in the header.
#[test]
fn inception_is_refused_and_the_header_names_the_real_path() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    let mut req = ScaffoldRequest {
        layer: "domain".into(),
        container: "budget".into(),
        name: "budget".into(),
        kind: ScaffoldKind::Primitive,
        path: None,
        description: "budgets".into(),
        role: None,
    };
    let err = run(&root, &store, &req).unwrap_err().to_string();
    assert!(err.contains("module_inception"), "{err}");
    assert!(!root.join("src/domain/budget/budget.rs").exists());

    req.name = "period".into();
    req.path = Some("crates/domain/src/budget/period".into());
    run(&root, &store, &req).unwrap();
    let body = std::fs::read_to_string(root.join("crates/domain/src/budget/period.rs")).unwrap();
    assert!(body.contains("crates/domain/src/budget"), "{body}");
    assert!(!body.contains("src/domain/budget"), "{body}");
}

/// household field test (0.3.50): in a Cargo workspace a new container
/// landed in `src/<layer>/<container>` at the repo root. Now it follows the
/// layer's crate, and the first container of a crate-less layer asks for
/// --path instead of inventing a stray tree.
#[test]
fn new_containers_follow_the_cargo_layout() {
    let root = tmp_dir();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("crates/app/src")).unwrap();
    std::fs::write(
        root.join("crates/app/Cargo.toml"),
        "[package]\nname = \"app\"\n",
    )
    .unwrap();
    std::fs::write(root.join("crates/app/src/lib.rs"), "").unwrap();
    let store = Store::open(&root).unwrap();
    let mut req = ScaffoldRequest {
        layer: "app".into(),
        container: "reports".into(),
        name: "monthly".into(),
        kind: ScaffoldKind::Primitive,
        path: None,
        description: String::new(),
        role: None,
    };
    let err = run(&root, &store, &req).unwrap_err().to_string();
    assert!(
        err.contains("--path crates/app/src/reports/monthly"),
        "{err}"
    );
    assert!(!root.join("src").exists(), "no stray root tree");

    req.path = Some("crates/app/src/reports/monthly".into());
    run(&root, &store, &req).unwrap();
    // a sibling container of the same layer is born in the same crate
    req.container = "imports".into();
    req.name = "csv".into();
    req.path = None;
    run(&root, &store, &req).unwrap();
    assert!(root.join("crates/app/src/imports/csv.rs").exists());
}

/// household field test (0.3.50): the data loader's wiring did not compile
/// as pasted — `fn load_currency-table()`, an `include_str!` path relative
/// to the wrong place, and an `.expect` that broke the project's no-panic
/// rule.
#[test]
fn the_data_loader_wiring_compiles_as_pasted() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    let req = ScaffoldRequest {
        layer: "domain".into(),
        container: "money".into(),
        name: "currency-table".into(),
        kind: ScaffoldKind::Data,
        path: None,
        description: "ISO currencies".into(),
        role: None,
    };
    run(&root, &store, &req).unwrap();
    let loader =
        std::fs::read_to_string(root.join("src/domain/money/currency_table_loader.rs")).unwrap();
    assert!(loader.contains("pub fn load_currency_table()"), "{loader}");
    assert!(
        loader.contains("include_str!(\"currency_table.toml\")"),
        "{loader}"
    );
    assert!(loader.contains("Result<Vec<"), "{loader}");
    assert!(!loader.contains(".expect("), "{loader}");
}

/// 0.4.1: scaffolding a component with a planned slot's name fills the
/// slot at its declared path, and the slot gives way to a real component.
#[test]
fn scaffold_fills_a_planned_slot_at_its_path() {
    let root = tmp_dir();
    let store = Store::open(&root).unwrap();
    store.init().unwrap();
    let mut m = store.load_manifest().unwrap();
    m.layers.push(crate::schema::model::manifest::Layer {
        name: "app".into(),
        containers: vec![crate::schema::model::manifest::Container {
            name: "cache".into(),
            path: "src/app/cache".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    crate::generation::structure::edit::slots::add_slot(
        &mut m,
        "cache",
        "warm-cache",
        "src/app/cache/warm.rs",
        "read-through cache",
    )
    .unwrap();
    store.save_manifest(&m).unwrap();
    let req = ScaffoldRequest {
        layer: "app".into(),
        container: "cache".into(),
        name: "warm-cache".into(),
        kind: ScaffoldKind::Primitive,
        path: None,
        description: "read-through cache".into(),
        role: None,
    };
    run(&root, &store, &req).unwrap();
    assert!(
        root.join("src/app/cache/warm.rs").exists(),
        "built at the slot's path"
    );
    let m = store.load_manifest().unwrap();
    let comps = &m
        .layers
        .iter()
        .find(|l| l.name == "app")
        .unwrap()
        .containers[0]
        .components;
    assert_eq!(comps.len(), 1, "{comps:?}");
    assert!(!comps[0].planned);
}
