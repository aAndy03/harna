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
        ""
    )
    .is_err());
    assert!(add(&mut m, "it", &[], &[], "").is_err());
    add(
        &mut m,
        "it",
        &[r"crates\api\tests\**".into()],
        &["store".into()],
        "",
    )
    .unwrap();
    add(
        &mut m,
        "it",
        &["crates/*/tests/**".into()],
        &["*".into()],
        "",
    )
    .unwrap();
    assert_eq!(m.test_kinds.len(), 1);
    assert_eq!(m.test_kinds[0].paths, vec!["crates/*/tests/**"]);
    let mut n = manifest();
    add(&mut n, "w", &[r"crates\api\tests\**".into()], &[], "").unwrap();
    assert_eq!(n.test_kinds[0].paths, vec!["crates/api/tests/**"]);
    assert!(remove(&mut n, "w").is_ok());
    assert!(remove(&mut n, "w").is_err());
}
