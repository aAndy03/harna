use super::*;

fn q(id: &str, scope: &[&str]) -> Quality {
    Quality {
        held_by: Vec::new(),
        id: id.into(),
        statement: format!("{id} holds"),
        measure: String::new(),
        category: String::new(),
        scope: scope.iter().map(|s| s.to_string()).collect(),
        why: String::new(),
        recorded: String::new(),
    }
}

/// A multi-user bookkeeping product: scale is system-wide, latency binds
/// the API, durability the storage. Code under `crates/api/src/routes`
/// meets the system-wide and the API qualities, never the storage one.
#[test]
fn qualities_reach_the_code_they_bind() {
    let mut all = Qualities::default();
    all.upsert(q("scale-users", &[])).unwrap();
    all.upsert(q("api-latency", &["crates/api"])).unwrap();
    all.upsert(q("no-data-loss", &["crates/storage"])).unwrap();
    let ids = |v: Vec<&Quality>| v.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
    assert_eq!(
        ids(all.in_scope(&["crates/api/src/routes"])),
        vec!["api-latency", "scale-users"]
    );
    assert_eq!(ids(all.in_scope(&[])), vec!["scale-users"]);
    // a container above the scope still meets it
    assert_eq!(ids(all.in_scope(&["crates"])).len(), 3);
}

#[test]
fn upsert_refines_and_remove_forgets() {
    let mut all = Qualities::default();
    all.upsert(q("offline", &[])).unwrap();
    let mut refined = q("OFFLINE", &[]);
    refined.measure = "100% of features without network".into();
    all.upsert(refined).unwrap();
    assert_eq!(all.qualities.len(), 1);
    assert!(all.qualities[0]
        .line()
        .contains("(100% of features without network)"));
    assert!(all.remove("offline"));
    assert!(all.is_empty());
}
