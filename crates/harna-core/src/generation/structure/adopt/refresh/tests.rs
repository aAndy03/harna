use super::*;
use crate::schema::model::manifest::{ComponentKind, Container, Layer};
use crate::schema::store::Store;

fn repo(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "harna-refresh-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/backend/index")).unwrap();
    std::fs::create_dir_all(root.join("src/ui")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"fm\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let files = [
        ("src/main.rs", "mod backend;\nmod ui;\nfn main() {}\n"),
        ("src/backend/mod.rs", "pub mod index;\n"),
        (
            "src/backend/index/mod.rs",
            "pub mod families;\npub mod search;\n",
        ),
        ("src/backend/index/families.rs", "pub fn group() {}\n"),
        (
            "src/backend/index/search.rs",
            "pub fn find() {}\n#[cfg(test)]\nmod tests;\n",
        ),
        ("src/backend/index/search/tests.rs", "#[test]\nfn t() {}\n"),
        ("src/ui/mod.rs", "pub mod window;\n"),
        ("src/ui/window.rs", "pub fn draw() {}\n"),
    ];
    for (p, src) in files {
        let abs = root.join(p);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(abs, src).unwrap();
    }
    root
}

fn comp(name: &str, path: &str) -> Component {
    Component {
        name: name.into(),
        kind: ComponentKind::Primitive,
        path: path.into(),
        summary: String::new(),
        tests: Vec::new(),
        provides: Vec::new(),
        children: Vec::new(),
    }
}

/// The font-manager shape after a week: `search.rs` arrived after
/// adoption, `catalog.rs` was moved away by hand, `ui` never got a path.
fn adopted() -> Manifest {
    Manifest {
        schema_version: 1,
        layers: vec![
            Layer {
                name: "backend".into(),
                containers: vec![Container {
                    name: "index".into(),
                    path: "src/backend/index".into(),
                    components: vec![
                        comp("families", "src/backend/index/families.rs"),
                        comp("catalog", "src/backend/index/catalog.rs"),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            },
            Layer {
                name: "ui".into(),
                containers: vec![Container {
                    name: "app".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
    }
}

/// ledgerly (0.3.43): `harna adopt` refused on an adopted repo, so new
/// modules drifted out of the map one hand edit at a time.
#[test]
fn refresh_proposes_new_modules_and_names_what_drifted() {
    let root = repo("drift");
    let r = refresh(&root, &adopted(), 1_000).unwrap();

    let key = ("index".to_string(), "src/backend/index".to_string());
    let proposed = &r.unregistered[&key];
    assert_eq!(proposed.len(), 1, "{r:?}");
    assert_eq!(proposed[0].name, "search");
    assert_eq!(proposed[0].path, "src/backend/index/search.rs");
    assert_eq!(proposed[0].tests, vec!["src/backend/index/search/tests.rs"]);
    assert_eq!(r.outside, vec!["src/ui/window.rs"], "glue never proposed");
    assert_eq!(
        r.stale,
        vec![(
            "catalog".to_string(),
            "src/backend/index/catalog.rs".to_string()
        )]
    );
    assert_eq!(r.pathless, vec![("app".to_string(), "ui".to_string())]);

    let text = r.render();
    assert!(text.contains("name = \"search\""), "{text}");
    assert!(text.contains("tests = [\"src/backend/index/search/tests.rs\"]"));
    assert!(text.contains("component `catalog`"));
    assert!(text.contains("harna container set app --path <dir>"));
    assert!(r
        .summary()
        .starts_with("refresh: 1 unregistered module(s), 1 outside"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn adopt_run_drafts_fresh_and_refreshes_adopted_repos() {
    let root = repo("run");
    let store = Store::open(&root).unwrap();
    let (summary, body) = crate::generation::structure::adopt::run(&root, &store, 1_000).unwrap();
    assert!(summary.starts_with("draft:"), "{summary}");
    assert!(body.contains("[[layers]]"));

    store.init().unwrap();
    store.save_manifest(&adopted()).unwrap();
    let (summary, _) = crate::generation::structure::adopt::run(&root, &store, 1_000).unwrap();
    assert!(summary.starts_with("refresh:"), "{summary}");

    // A name taken anywhere in the manifest gets its folder prefixed.
    let mut m = adopted();
    m.layers[1].containers[0]
        .components
        .push(comp("search", "src/ui/search.rs"));
    let r = refresh(&root, &m, 1_000).unwrap();
    let key = ("index".to_string(), "src/backend/index".to_string());
    assert_eq!(r.unregistered[&key][0].name, "index-search");
    let _ = std::fs::remove_dir_all(&root);
}
