use super::*;
use crate::enforcement::parity::history::{carry, HISTORY_CAP};

fn target(path: &str, status: &str) -> ParityTarget {
    ParityTarget {
        path: path.into(),
        reason: "imports a module with a breaking change".into(),
        status: status.into(),
        note: String::new(),
        by: String::new(),
        when: String::new(),
        breaking: true,
        history: Vec::new(),
    }
}

fn store_with(tag: &str, targets: Vec<ParityTarget>) -> (std::path::PathBuf, Store) {
    let root = std::env::temp_dir().join(format!(
        "harna-parity-amend-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let store = Store::open(&root).unwrap();
    store.init().unwrap();
    let ev = ParityEvent {
        id: "split".into(),
        kind: "digest-diff".into(),
        source: "test".into(),
        generation: 1,
        baseline_ref: "HEAD".into(),
        created: "2026-10-18".into(),
        updated: "2026-10-18".into(),
        targets,
    };
    save_event(&store, &ev).unwrap();
    (root, store)
}

/// ledgerly finding 9 (0.3.43): a corrected verdict (7 ack / 3 noop
/// became 6 / 4) overwrote the original note with no trace. The earlier
/// verdict is now kept, rendered, and survives the bulk path too.
#[test]
fn a_re_resolve_keeps_the_earlier_verdict_as_history() {
    let (root, store) = store_with(
        "one",
        vec![target("src/a.rs", "open"), target("src/b.rs", "open")],
    );
    resolve(
        &store,
        "split",
        "src/a.rs",
        "ack",
        "callers updated",
        "agent",
    )
    .unwrap();
    let ev = event(&store, "split").unwrap();
    assert!(
        ev.target("src/a.rs").unwrap().history.is_empty(),
        "open -> ack loses nothing"
    );

    let ev = resolve(
        &store,
        "split",
        "src/a.rs",
        "noop",
        "no caller uses it",
        "reviewer",
    )
    .unwrap();
    let t = ev.target("src/a.rs").unwrap();
    assert_eq!((t.status.as_str(), t.by.as_str()), ("noop", "reviewer"));
    assert_eq!(t.history.len(), 1);
    assert_eq!(t.history[0].status, "ack");
    assert_eq!(t.history[0].note, "callers updated");
    assert_eq!(t.history[0].by, "agent");
    let shown = ev.render();
    assert!(shown.contains("earlier: ack by agent @ "), "{shown}");
    assert!(shown.contains(": callers updated"), "{shown}");

    let entries = vec![(
        "src/a.rs".to_string(),
        "ack".to_string(),
        "re-checked".to_string(),
    )];
    let (ev, missing) = resolve_bulk(&store, "split", &entries, "agent").unwrap();
    assert!(missing.is_empty());
    let h = &ev.target("src/a.rs").unwrap().history;
    let trail: Vec<&str> = h.iter().map(|a| a.status.as_str()).collect();
    assert_eq!(trail, vec!["ack", "noop"], "oldest first");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn history_is_capped_and_carried_across_a_re_open() {
    let mut t = target("src/a.rs", "open");
    for i in 0..12 {
        amend(
            &mut t,
            "ack",
            &format!("pass {i}"),
            "agent",
            "2026-10-18T00:00:00",
        );
    }
    assert_eq!(t.history.len(), HISTORY_CAP);
    assert_eq!(t.history[0].note, "pass 3", "oldest dropped first");

    // Re-open: the new generation reopens the target; the hand verdict
    // joins the carried history instead of vanishing.
    let carried = carry(&t.history, previous(&t), "open");
    assert_eq!(carried.len(), HISTORY_CAP);
    assert_eq!(carried.last().unwrap().note, "pass 11");
    // A kept verdict (same status) is not duplicated into history.
    assert_eq!(carry(&t.history, previous(&t), "ack").len(), HISTORY_CAP);
}
