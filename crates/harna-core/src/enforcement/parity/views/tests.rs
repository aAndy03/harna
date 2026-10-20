use super::*;

fn target(path: &str, status: &str) -> ParityTarget {
    ParityTarget {
        path: path.into(),
        reason: String::new(),
        status: status.into(),
        note: String::new(),
        by: String::new(),
        when: String::new(),
        breaking: false,
        history: Vec::new(),
    }
}

fn event(id: &str, statuses: &[&str]) -> ParityEvent {
    ParityEvent {
        schema_version: crate::SCHEMA_BASELINE,
        id: id.into(),
        kind: "digest-diff".into(),
        source: "test".into(),
        generation: 1,
        baseline_ref: "HEAD".into(),
        created: "2026-10-18".into(),
        updated: "2026-10-18".into(),
        targets: statuses
            .iter()
            .enumerate()
            .map(|(i, s)| target(&format!("src/m{i}.rs"), s))
            .collect(),
    }
}

/// font-manager (0.3.42): "0 open / 7 targets" read as "no targets", and
/// closed events kept printing in full as the ledger grew.
#[test]
fn summary_leads_with_the_fan_out_and_closed_events_collapse() {
    let closed = event("perf-preview", &["origin", "origin", "auto", "noop"]);
    let s = closed.summary();
    assert!(s.contains("0 open — nothing owed (of 4 checked"), "{s}");
    assert!(
        s.contains("2 origin (your change), 1 auto-resolved, 1 hand-resolved"),
        "{s}"
    );

    let live = event("collections", &["origin", "open"]);
    assert!(live.summary().contains("1 open — owed: ack or noop each"));

    let root = std::env::temp_dir().join(format!(
        "harna-parity-views-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let store = Store::open(&root).unwrap();
    store.init().unwrap();
    for e in [&closed, &live] {
        save_event(&store, e).unwrap();
    }
    let all = show(&store, None, false).unwrap();
    assert!(
        all.contains("perf-preview (digest-diff): closed — 4 target(s)"),
        "{all}"
    );
    assert!(!all.contains("src/m3.rs"), "closed detail collapsed: {all}");
    assert!(all.contains("! open"), "live event in full: {all}");
    let one = show(&store, Some("perf-preview"), false).unwrap();
    assert!(one.contains("src/m3.rs"), "by id, detail stays: {one}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn origin_notes_do_not_call_your_own_change_breaking() {
    let p = policy::place(&["signature removed: pub fn load()".into()], &[], None, 1);
    assert!(!p.note.contains("(breaking)"), "{}", p.note);
    assert!(p.note.contains("importers are reviewed"), "{}", p.note);
}
