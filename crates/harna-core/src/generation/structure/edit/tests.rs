use super::*;

fn container(name: &str, path: &str) -> Container {
    Container {
        name: name.into(),
        path: path.into(),
        ..Default::default()
    }
}

/// The font-manager shape before its rename: `catalog` moved to
/// `src/backend/index` with `harna move`, but its name stayed `catalog`.
fn font_manager() -> Manifest {
    Manifest {
        schema_version: 1,
        layers: vec![
            Layer {
                name: "backend".into(),
                sequence: vec!["model".into(), "catalog".into()],
                containers: vec![
                    container("model", "src/backend/model"),
                    container("catalog", "src/backend/index"),
                ],
                ..Default::default()
            },
            Layer {
                name: "ui".into(),
                containers: vec![container("app", "src/ui/app")],
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

#[test]
fn rename_relinks_the_layer_sequence_and_refuses_collisions() {
    let mut m = font_manager();
    let changes = rename_container(&mut m, "catalog", "index").unwrap();
    assert_eq!(m.layers[0].containers[1].name, "index");
    assert_eq!(m.layers[0].sequence, vec!["model", "index"]);
    assert!(changes[0].contains("`catalog` -> `index`"), "{changes:?}");
    assert!(changes.iter().any(|c| c.contains("sequence relinked")));

    let err = rename_container(&mut m, "index", "app").unwrap_err();
    assert!(err.to_string().contains("already exists"), "{err}");
    let err = rename_container(&mut m, "catalog", "x").unwrap_err();
    assert!(
        err.to_string().contains("no container named `catalog`"),
        "{err}"
    );
}

#[test]
fn container_set_writes_path_gateway_and_role_and_clears_with_empty() {
    let mut m = font_manager();
    let set = ContainerSet {
        owners: None,
        provides: None,
        escapes: None,
        owns: None,
        path: Some("src\\backend\\discovery/".into()),
        gateway: Some("fs".into()),
        role: Some("gateway".into()),
        description: None,
    };
    let changes = set_container(&mut m, "model", &set).unwrap();
    let c = &m.layers[0].containers[0];
    assert_eq!(c.path, "src/backend/discovery");
    assert_eq!(c.gateway.as_deref(), Some("fs"));
    assert_eq!(c.role.as_deref(), Some("gateway"));
    assert_eq!(changes.len(), 3);

    let clear = ContainerSet {
        gateway: Some(String::new()),
        ..Default::default()
    };
    set_container(&mut m, "model", &clear).unwrap();
    assert_eq!(m.layers[0].containers[0].gateway, None);

    let bad = ContainerSet {
        role: Some("brain".into()),
        ..Default::default()
    };
    let err = set_container(&mut m, "model", &bad).unwrap_err();
    assert!(err.to_string().contains("unknown role `brain`"), "{err}");
}

/// font-manager: `ui` imported `backend` with no `depends_on` declared; the
/// agent hand-edited the manifest. `layer set` declares it, checked.
#[test]
fn layer_set_declares_depends_on_against_known_layers() {
    let mut m = font_manager();
    let set = LayerSet {
        depends_on: Some(split_list("backend")),
        ..Default::default()
    };
    let changes = set_layer(&mut m, "ui", &set).unwrap();
    assert_eq!(m.layers[1].depends_on, vec!["backend"]);
    assert!(changes[0].contains("depends_on [] -> [backend]"));

    for (deps, why) in [("ui", "itself"), ("frontend", "no layer named `frontend`")] {
        let set = LayerSet {
            depends_on: Some(split_list(deps)),
            ..Default::default()
        };
        let err = set_layer(&mut m, "ui", &set).unwrap_err();
        assert!(err.to_string().contains(why), "{err}");
    }
    assert_eq!(
        m.layers[1].depends_on,
        vec!["backend"],
        "refusals change nothing"
    );
    assert!(split_list("").is_empty(), "empty clears");
}

/// 0.3.45: what adopt's refresh proposes, registered without a hand edit
/// — the name from the file stem, the tests that exist, the sequence
/// extended, a clash named.
#[test]
fn register_turns_a_refresh_proposal_into_one_command() {
    let root = std::env::temp_dir().join(format!(
        "harna-register-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/backend/index/search")).unwrap();
    std::fs::write(
        root.join("src/backend/index/search.rs"),
        "pub fn f() {}\n#[cfg(test)]\nmod tests;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/backend/index/search/tests.rs"),
        "#[test]\nfn t() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/backend/index/families.rs"),
        "pub fn g() {}\n",
    )
    .unwrap();
    let mut m = font_manager();
    m.layers[0].containers[1].sequence = vec!["families".into()];
    let changes = register_component(
        &root,
        &mut m,
        "catalog",
        "src/backend/index/search.rs",
        None,
        None,
    )
    .unwrap();
    let c = &m.layers[0].containers[1];
    let comp = c.components.iter().find(|x| x.name == "search").unwrap();
    assert_eq!(comp.tests, vec!["src/backend/index/search/tests.rs"]);
    assert_eq!(c.sequence, vec!["families", "search"]);
    assert!(changes.iter().any(|l| l.contains("no summary yet")));

    let err = register_component(
        &root,
        &mut m,
        "catalog",
        "src/backend/index/search.rs",
        None,
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("already registered"), "{err}");
    let err = register_component(&root, &mut m, "catalog", "src/nope.rs", None, None).unwrap_err();
    assert!(err.to_string().contains("not a file"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

/// ledger field test (0.3.49): `crates/storage/tests/repos.rs` covers every
/// repository component; registering it took a hand edit of the manifest.
#[test]
fn tests_register_for_a_whole_container_or_named_components() {
    let root = std::env::temp_dir().join(format!(
        "harna-edit-tests-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("crates/storage/tests")).unwrap();
    std::fs::write(root.join("crates/storage/tests/repos.rs"), "").unwrap();
    let comp = |n: &str| crate::schema::model::manifest::Component {
        name: n.into(),
        path: format!("crates/storage/src/{n}.rs"),
        ..Default::default()
    };
    let mut m = Manifest {
        layers: vec![Layer {
            name: "storage".into(),
            containers: vec![Container {
                name: "sqlite".into(),
                components: vec![comp("accounts-repo"), comp("tx-repo")],
                ..container("sqlite", "crates/storage/src")
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let add = vec!["crates/storage/tests/repos.rs".to_string()];
    let changes = set_tests(&root, &mut m, "sqlite", &[], &add, &[]).unwrap();
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert!(m.layers[0].containers[0]
        .components
        .iter()
        .all(|c| c.tests == add));
    let only = vec!["tx-repo".to_string()];
    set_tests(&root, &mut m, "sqlite", &only, &[], &add).unwrap();
    assert!(m.layers[0].containers[0].components[1].tests.is_empty());
    assert_eq!(m.layers[0].containers[0].components[0].tests, add);
    assert!(set_tests(&root, &mut m, "sqlite", &[], &["nope.rs".into()], &[]).is_err());
    assert!(set_tests(&root, &mut m, "sqlite", &["ghost".into()], &add, &[]).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

/// household field test (0.3.50): a scaffold into the wrong place could
/// only be undone by hand-editing the manifest.
#[test]
fn containers_and_layers_come_out_of_the_manifest_files_untouched() {
    let mut m = font_manager();
    m.layers[0].depends_on = vec![];
    m.layers.push(Layer {
        name: "ui".into(),
        depends_on: vec!["backend".into()],
        ..Default::default()
    });
    let out = remove_container(&mut m, "catalog").unwrap();
    assert!(
        out[0].contains("files under `src/backend/index` untouched"),
        "{out:?}"
    );
    assert!(!m.layers[0].sequence.contains(&"catalog".to_string()));
    assert!(remove_container(&mut m, "catalog").is_err());
    let out = remove_layer(&mut m, "backend").unwrap();
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(m.layers[0].depends_on.is_empty());
}
