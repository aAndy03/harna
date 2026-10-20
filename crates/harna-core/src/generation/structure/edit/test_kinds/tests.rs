use super::*;
use crate::schema::model::manifest::Layer;

fn manifest() -> Manifest {
    Manifest {
        layers: vec![Layer {
            name: "store".into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// 0.4.9: a reach naming no layer is refused (a typo would silently allow
/// nothing), Windows globs are normalized, and the same name replaces.
#[test]
fn a_test_kind_is_checked_normalized_and_replaced() {
    let mut m = manifest();
    assert!(add(
        &mut m,
        "it",
        &["crates/*/tests/**".into()],
        &["stor".into()],
        "",
        None
    )
    .is_err());
    assert!(add(&mut m, "it", &[], &[], "", None).is_err());
    add(
        &mut m,
        "it",
        &[r"crates\api\tests\**".into()],
        &["store".into()],
        "",
        None,
    )
    .unwrap();
    add(
        &mut m,
        "it",
        &["crates/*/tests/**".into()],
        &["*".into()],
        "",
        None,
    )
    .unwrap();
    assert_eq!(m.test_kinds.len(), 1);
    assert_eq!(m.test_kinds[0].paths, vec!["crates/*/tests/**"]);
    let mut n = manifest();
    add(&mut n, "w", &[r"crates\api\tests\**".into()], &[], "", None).unwrap();
    assert_eq!(n.test_kinds[0].paths, vec!["crates/api/tests/**"]);
    assert!(remove(&mut n, "w").is_ok());
    assert!(remove(&mut n, "w").is_err());
}

/// 0.5.8: a kind may require a marker in its files; a broken marker is
/// refused at add time.
#[test]
fn a_marker_narrows_the_kind() {
    let mut m = manifest();
    assert!(add(&mut m, "ui", &["tests/**".into()], &[], "", Some("(")).is_err());
    add(
        &mut m,
        "ui",
        &["tests/**".into()],
        &[],
        "",
        Some(r"#\[gpui_kit::test"),
    )
    .unwrap();
    let k = &m.test_kinds[0];
    assert!(k.covers_file("tests/a.rs", "#[gpui_kit::test]\nfn t() {}"));
    assert!(!k.covers_file("tests/b.rs", "#[test]\nfn t() {}"));
    assert!(!k.covers_file("src/a.rs", "#[gpui_kit::test]"));
}
