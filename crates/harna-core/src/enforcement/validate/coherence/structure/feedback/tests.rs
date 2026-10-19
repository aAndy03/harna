use super::*;

fn graph<'a>(edges: &[(&'a str, &'a str)]) -> BTreeMap<&'a str, Vec<&'a str>> {
    let mut g: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (a, b) in edges {
        g.entry(a).or_default().push(b);
        g.entry(b).or_default();
    }
    g
}

/// codex (0.3.48, H12): `analytics/events.rs` <-> `facts.rs` sits in many
/// rings. Three rings through one shared import are one finding.
#[test]
fn one_shared_import_is_one_finding_for_all_its_rings() {
    let g = graph(&[
        ("events", "facts"),
        ("facts", "events"),
        ("facts", "a"),
        ("a", "events"),
        ("facts", "b"),
        ("b", "events"),
    ]);
    let f = edges(&g);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!((f[0].from.as_str(), f[0].to.as_str()), ("events", "facts"));
    assert_eq!(f[0].rings, 3);
    assert_eq!(f[0].example.first(), f[0].example.last());
}

/// Disjoint rings need one edge each; the result leaves the graph acyclic.
#[test]
fn the_chosen_edges_leave_no_ring() {
    let g = graph(&[
        ("a", "b"),
        ("b", "a"),
        ("c", "d"),
        ("d", "e"),
        ("e", "c"),
        ("x", "y"),
    ]);
    let f = edges(&g);
    assert_eq!(f.len(), 2, "{f:?}");
    let mut cut: BTreeMap<String, BTreeSet<String>> = g
        .iter()
        .map(|(k, v)| (k.to_string(), v.iter().map(|t| t.to_string()).collect()))
        .collect();
    for e in &f {
        cut.get_mut(&e.from).unwrap().remove(&e.to);
    }
    assert!(rings(&cut).is_empty());
    assert!(edges(&graph(&[("x", "y"), ("y", "z")])).is_empty());
}
