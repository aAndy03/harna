use super::*;
use crate::enforcement::parity::fixtures::*;
use crate::schema::store::Store;

#[test]
fn auto_never_waives_a_breaking_importer() {
    let tmp = std::env::temp_dir().join(format!("harna-parity-auto-{}", std::process::id()));
    // Windows reuses process ids: a stale dir from a previous run would
    // carry old parity events in.
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let (baseline, fresh) = digests();
    let d = diff::diff(&baseline, &fresh);
    let event = open_from_digests(
        &store,
        "auto-test",
        "digest-diff",
        "",
        "HEAD",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();
    // The importer still imports `load`, which the change removed.
    let importer = event.target("crates/y/src/api.rs").unwrap();
    assert_eq!(
        (importer.status.as_str(), importer.breaking),
        ("open", true)
    );
    assert!(
        importer.reason.contains("signature removed: pub fn load()"),
        "{}",
        importer.reason
    );

    // Its own surface is identical to the baseline — before 0.3.35 that
    // auto-verified it. It is exactly the module that may now call
    // something gone, so it stays owed.
    let (ev, verified, open) = auto_from(&store, &event, &baseline, &fresh).unwrap();
    assert_eq!((verified, open), (0, 1));
    assert_eq!(ev.target("crates/y/src/api.rs").unwrap().status, "open");

    // Auto never opens new targets and never bumps the generation.
    assert_eq!(ev.generation, event.generation);
    assert_eq!(ev.targets.len(), event.targets.len());
}

#[test]
fn auto_flags_deleted_target_modules() {
    let tmp = std::env::temp_dir().join(format!("harna-parity-del-{}", std::process::id()));
    // Windows reuses process ids: a stale dir from a previous run would
    // carry old parity events in.
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let (baseline, fresh) = digests();
    let d = diff::diff(&baseline, &fresh);
    let event = open_from_digests(
        &store,
        "del-test",
        "digest-diff",
        "",
        "HEAD",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();

    // A tree where the importer module is deleted entirely.
    let aftermath = Digest {
        generated_at: "later".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: fresh
            .modules
            .iter()
            .filter(|m| !m.path.ends_with("api.rs"))
            .cloned()
            .collect(),
    };
    let (ev, _v, open) = auto_from(&store, &event, &baseline, &aftermath).unwrap();
    let importer = ev
        .target("crates/y/src/api.rs")
        .expect("target kept visible");
    assert_eq!(
        importer.status, "open",
        "deletion debt is never auto-cleared"
    );
    assert!(importer.reason.contains("module deleted from current tree"));
    assert!(open >= 1);
}

#[test]
fn auto_pass_sweeps_every_event() {
    let tmp = std::env::temp_dir().join(format!("harna-parity-sweep-{}", std::process::id()));
    // Windows reuses process ids: a stale dir from a previous run would
    // carry old parity events in.
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let (baseline, fresh) = digests();
    let d = diff::diff(&baseline, &fresh);
    open_from_digests(
        &store,
        "sweep-a",
        "digest-diff",
        "",
        "HEAD",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();
    open_from_digests(
        &store,
        "sweep-b",
        "digest-diff",
        "",
        "HEAD",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();

    let lines = auto_sweep(&store, |_| Ok(baseline.clone()), &fresh).unwrap();
    assert_eq!(lines.len(), 2);
    assert!(
        lines
            .iter()
            .all(|l| l.contains("0 auto-verified, 1 still open")),
        "{lines:?}"
    );
    // Sweeping leaves both events with the breaking importer still owed.
    for id in ["sweep-a", "sweep-b"] {
        let ev = crate::enforcement::parity::list_events(&store)
            .unwrap()
            .into_iter()
            .find(|e| e.id == id)
            .unwrap();
        assert_eq!(ev.open_targets().len(), 1);
    }
}
