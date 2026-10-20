use super::*;
use crate::schema::model::manifest::{Container, Layer};

fn manifest() -> Manifest {
    Manifest {
        layers: vec![Layer {
            name: "app".into(),
            containers: vec![Container {
                name: "cache".into(),
                path: "crates/app/src/cache".into(),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// 0.4.1: "the cache will live here, behind this gateway" — declared
/// before its code; the missing file is intent, not drift; a second slot
/// of the same name is refused; filling needs the code to exist.
#[test]
fn a_slot_is_intent_until_its_code_exists() {
    let root = std::env::temp_dir().join(format!(
        "harna-slots-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut m = manifest();
    add_slot(
        &mut m,
        "cache",
        "warm-cache",
        "crates/app/src/cache/warm.rs",
        "read-through",
    )
    .unwrap();
    assert!(add_slot(&mut m, "cache", "warm-cache", "x.rs", "").is_err());
    assert_eq!(slots(&m).len(), 1);

    let mut out = Vec::new();
    crate::enforcement::validate::coherence::manifest::check_paths_exist(&root, &m, &mut out);
    assert!(
        out.is_empty(),
        "a slot (and its slot-only container) is intent: {out:?}"
    );

    assert!(
        fill_slot(&root, &mut m, "warm-cache").is_err(),
        "no code yet"
    );
    std::fs::create_dir_all(root.join("crates/app/src/cache")).unwrap();
    std::fs::write(root.join("crates/app/src/cache/warm.rs"), "").unwrap();
    let mut out = Vec::new();
    crate::enforcement::validate::coherence::manifest::check_paths_exist(&root, &m, &mut out);
    assert!(out[0].message.contains("now has its code"), "{out:?}");
    fill_slot(&root, &mut m, "warm-cache").unwrap();
    assert!(slots(&m).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}
