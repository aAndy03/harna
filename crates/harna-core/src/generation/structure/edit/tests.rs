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
