use super::*;
use crate::schema::model::manifest::{Container, Layer};

fn layer(name: &str, path: &str, depends: &[&str]) -> Layer {
    Layer {
        name: name.into(),
        depends_on: depends.iter().map(|s| s.to_string()).collect(),
        containers: vec![Container {
            name: name.into(),
            path: path.into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// deno study B4 (0.3.45): "Each layer depends only on the ones below
/// it" is prose in doc/architecture.md; the Cargo graph is where it can
/// be held exactly. An ext crate depending back on the CLI is named.
#[test]
fn cargo_dependencies_follow_the_layer_dag() {
    let root = std::env::temp_dir().join(format!(
        "harna-cargo-layers-{}-{:?}",
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
        "[workspace]\nmembers = [\"cli\", \"ext/fs\", \"libs/core\"]\n\
         [workspace.dependencies]\ndeno_core = { path = \"./libs/core\" }\n\
         deno_fs = { path = \"./ext/fs\" }\ndeno = { path = \"./cli\" }\n",
    );
    w(
        "cli/Cargo.toml",
        "[package]\nname = \"deno\"\n[lib]\npath = \"lib.rs\"\n\
         [dependencies]\ndeno_fs.workspace = true\ndeno_core = { path = \"../libs/core\" }\nserde = \"1\"\n",
    );
    w(
        "ext/fs/Cargo.toml",
        "[package]\nname = \"deno_fs\"\n[lib]\npath = \"lib.rs\"\n\
         [dependencies]\ndeno_core.workspace = true\ndeno.workspace = true\n\
         [dev-dependencies]\ndeno.workspace = true\n",
    );
    w(
        "libs/core/Cargo.toml",
        "[package]\nname = \"deno_core\"\n[lib]\npath = \"lib.rs\"\n",
    );
    let manifest = Manifest {
        schema_version: 1,
        layers: vec![
            layer("cli", "cli", &["ext", "libs"]),
            layer("ext", "ext/fs", &["libs"]),
            layer("libs", "libs/core", &[]),
        ],
        ..Default::default()
    };
    let mut out = Vec::new();
    check(&root, &manifest, &mut out);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].file, "ext/fs/Cargo.toml");
    assert!(
        out[0].message.contains("`deno` reaches layer(s) cli"),
        "{}",
        out[0].message
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// gpui field test (0.4.3): `finance-summary` depending on its sibling
/// `finance-transactions` passed while the same `use` was caught. An
/// isolated layer holds in Cargo.toml too.
#[test]
fn isolated_siblings_may_not_depend_on_each_other() {
    let root = std::env::temp_dir().join(format!(
        "harna-cargo-iso-{}-{:?}",
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
    w("crates/tx/Cargo.toml", "[package]\nname = \"tx\"\n");
    w("crates/tx/src/lib.rs", "");
    w(
        "crates/summary/Cargo.toml",
        "[package]\nname = \"summary\"\n[dependencies]\ntx = { path = \"../tx\" }\n",
    );
    w("crates/summary/src/lib.rs", "");
    let c = |name: &str, path: &str| crate::schema::model::manifest::Container {
        name: name.into(),
        path: path.into(),
        ..Default::default()
    };
    let mut m = Manifest {
        layers: vec![crate::schema::model::manifest::Layer {
            name: "features".into(),
            containers: vec![c("tx", "crates/tx/src"), c("summary", "crates/summary/src")],
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut out = Vec::new();
    check(&root, &m, &mut out);
    assert!(out.is_empty(), "not isolated: {out:?}");
    m.layers[0].isolated = true;
    let mut out = Vec::new();
    check(&root, &m, &mut out);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].check, "sibling-isolation");
    assert_eq!(out[0].file, "crates/summary/Cargo.toml");
    // and deny.toml no longer lists siblings as each other's wrappers
    let plan = crate::generation::views::deny::plan(&root, &m).unwrap();
    assert!(
        plan.rules.iter().all(|r| r.wrappers.is_empty()),
        "{:?}",
        plan.rules
    );
    let _ = std::fs::remove_dir_all(&root);
}
