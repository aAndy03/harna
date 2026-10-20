use super::*;
use crate::schema::model::manifest::{Container, Layer};
use crate::search::digest::{Item, Module};

fn module(path: &str, reexports: &[&str], items: &[(&str, &str, &str)]) -> Module {
    Module {
        path: path.into(),
        lang: "rust".into(),
        signatures: vec![],
        deps: vec![],
        body_hash: String::new(),
        inventory: items
            .iter()
            .map(|(vis, kind, name)| Item {
                vis: vis.to_string(),
                kind: kind.to_string(),
                name: name.to_string(),
                text: String::new(),
            })
            .collect(),
        mods: vec![],
        crate_dir: Some("crates/domain".into()),
        src_root: Some("crates/domain/src".into()),
        reexports: reexports.iter().map(|s| s.to_string()).collect(),
    }
}

fn manifest(path: &str) -> Manifest {
    Manifest {
        layers: vec![Layer {
            name: "domain".into(),
            containers: vec![Container {
                name: "money".into(),
                path: path.into(),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// household field test (0.4.5): the surface was typed twice — once as
/// `pub use` in mod.rs, once as `--provides`. The draft reads the first.
#[test]
fn the_draft_is_the_root_modules_reexports() {
    let d = Digest {
        modules: vec![
            module(
                "crates/domain/src/money/mod.rs",
                &["Money", "Currency", "*"],
                &[],
            ),
            module(
                "crates/domain/src/money/round.rs",
                &[],
                &[("pub", "fn", "round_half_even"), ("priv", "fn", "helper")],
            ),
        ],
        ..Default::default()
    };
    let m = manifest("crates/domain/src/money");
    let (source, names) = draft(&m, &d, "money").unwrap();
    assert!(source.contains("re-exports"));
    assert_eq!(names, vec!["Currency", "Money"]);
    // without re-exports: the public items, never the private ones
    let d2 = Digest {
        modules: vec![d.modules[1].clone()],
        ..Default::default()
    };
    let (source, names) = draft(&m, &d2, "money").unwrap();
    assert!(source.contains("public items"));
    assert_eq!(names, vec!["round_half_even"]);
    let mut m = m;
    apply(&mut m, "money", &names).unwrap();
    assert_eq!(m.layers[0].containers[0].provides, names);
}
