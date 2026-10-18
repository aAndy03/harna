use super::*;
use crate::schema::store::Store;
use crate::search::digest::Module;

#[test]
fn traversing_event_ids_never_reach_the_disk() {
    let root = std::env::temp_dir().join(format!("harna-parity-id-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let store = Store::open(&root).unwrap();
    store.init().unwrap();
    let manifest_before = std::fs::read_to_string(store.manifest_path()).unwrap();
    let d = DigestDiff::default();
    for bad in ["../manifest", "..\\manifest", "a/b", ".hidden", "C:/x"] {
        let r = open_from_digests(
            &store,
            bad,
            "manual",
            "test",
            "HEAD",
            &Digest::default(),
            &Digest::default(),
            &d,
            &[],
        );
        assert!(r.is_err(), "`{bad}` must be refused");
        assert!(event(&store, bad).is_err());
    }
    assert_eq!(
        std::fs::read_to_string(store.manifest_path()).unwrap(),
        manifest_before,
        "the manifest keeps its bytes"
    );
    for good in ["harna-0-3-32", "fix_x", "v0.3.33"] {
        assert!(check_id(good).is_ok(), "{good}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Workspace crate names for the fixture digests (crate `x` is a member
/// so cross-crate imports like `x::core::store::load` resolve).
fn workspace() -> Vec<String> {
    vec!["x".to_string()]
}

fn module(path: &str, sigs: &[&str], deps: &[&str]) -> Module {
    Module {
        path: path.into(),
        lang: "rust".into(),
        signatures: sigs.iter().map(|s| s.to_string()).collect(),
        deps: deps.iter().map(|s| s.to_string()).collect(),
        body_hash: "fixture".into(),
        inventory: vec![],
        mods: vec![],
    }
}

fn digests() -> (Digest, Digest) {
    let baseline = Digest {
        generated_at: "before".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![
            module("crates/x/src/core/store.rs", &["pub fn load()"], &[]),
            // API picks up x's store via the workspace crate
            module("crates/y/src/api.rs", &[], &["x::core::store::load"]),
            module("crates/x/src/core/logger.rs", &["pub fn log()"], &[]),
        ],
    };
    let fresh = Digest {
        generated_at: "after".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![
            module("crates/x/src/core/store.rs", &["pub fn load_v2()"], &[]),
            module("crates/y/src/api.rs", &[], &["x::core::store::load"]),
            module("crates/x/src/core/logger.rs", &["pub fn log()"], &[]),
            module("crates/x/src/core/added.rs", &["pub fn new_thing()"], &[]),
        ],
    };
    (baseline, fresh)
}

#[test]
fn fan_out_is_coupling_not_distance() {
    let tmp = std::env::temp_dir().join(format!("harna-parity-fanout-{}", std::process::id()));
    // Windows reuses process ids: a stale dir from a previous run would
    // carry old parity events in.
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let (baseline, fresh) = digests();
    let d = diff::diff(&baseline, &fresh);
    let event = open_from_digests(
        &store,
        "store-change",
        "digest-diff",
        "store: load -> load_v2",
        "test-ref",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();

    // changed: store.rs (sig) + added.rs (new module)
    // importer: crates/y/src/api.rs imports workspace crate x's store
    // NOT a target: logger.rs — physically adjacent, structurally untouched.
    let paths: Vec<&str> = event.targets.iter().map(|t| t.path.as_str()).collect();
    assert!(paths.contains(&"crates/x/src/core/store.rs"));
    assert!(paths.contains(&"crates/x/src/core/added.rs"));
    assert!(
        paths.contains(&"crates/y/src/api.rs"),
        "importer enters ring 2"
    );
    assert!(
        !paths.contains(&"crates/x/src/core/logger.rs"),
        "no geometric spread"
    );
}

#[test]
fn revision_invalidates_acks_and_auto_verifies_unchanged() {
    let tmp = std::env::temp_dir().join(format!("harna-parity-rev-{}", std::process::id()));
    // Windows reuses process ids: a stale dir from a previous run would
    // carry old parity events in.
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let (baseline, fresh) = digests();
    let d = diff::diff(&baseline, &fresh);
    let mut event = open_from_digests(
        &store,
        "rev-test",
        "digest-diff",
        "",
        "gen1",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();
    assert_eq!(event.generation, 1);
    // 0.3.35: only the importer of the breaking rename (load -> load_v2)
    // is owed; the origins carry the commit as their record.
    assert_eq!(event.open_targets().len(), 1);
    event = resolve(
        &store,
        "rev-test",
        "crates/y/src/api.rs",
        "ack",
        "moved the call to load_v2",
        "user",
    )
    .unwrap();
    assert!(event.open_targets().is_empty());

    // Revision 2, same breakage: the importer's review still answers it,
    // so its verdict carries; the origins stay origins.
    let event2 = open_from_digests(
        &store,
        "rev-test",
        "digest-diff",
        "",
        "gen2",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();
    assert_eq!(event2.generation, 2);
    let store_target = event2
        .targets
        .iter()
        .find(|t| t.path.ends_with("store.rs"))
        .unwrap();
    let importer = event2
        .targets
        .iter()
        .find(|t| t.path.ends_with("api.rs"))
        .unwrap();
    assert_eq!(
        store_target.status, "origin",
        "the changed module is the origin"
    );
    assert_eq!(importer.status, "ack", "same breakage, same verdict");
    assert!(importer.note.contains("moved the call to load_v2"));
    assert!(importer.note.contains("carried from an earlier generation"));
    assert_eq!(importer.by, "user", "provenance carries with the verdict");
}

#[test]
fn origins_and_additive_importers_open_resolved() {
    let tmp = std::env::temp_dir().join(format!("harna-parity-additive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let (baseline, _) = digests();
    // Purely additive: store.rs gains a function, nothing is removed.
    let mut fresh = baseline.clone();
    fresh.modules[0]
        .signatures
        .push("pub fn load_many()".into());
    let d = diff::diff(&baseline, &fresh);
    let ev = open_from_digests(
        &store,
        "additive",
        "digest-diff",
        "",
        "HEAD",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();
    assert!(
        ev.open_targets().is_empty(),
        "an additive change owes nobody an ack: {:#?}",
        ev.targets
    );
    let origin = ev.target("crates/x/src/core/store.rs").unwrap();
    assert_eq!(origin.status, "origin");
    let importer = ev.target("crates/y/src/api.rs").unwrap();
    assert_eq!(
        (importer.status.as_str(), importer.breaking),
        ("auto", false)
    );
    assert!(importer.note.contains("nothing it imports was removed"));
}

#[test]
fn resolve_validates_status_and_target() {
    let tmp = std::env::temp_dir().join(format!("harna-parity-res-{}", std::process::id()));
    // Windows reuses process ids: a stale dir from a previous run would
    // carry old parity events in.
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let (baseline, fresh) = digests();
    let d = diff::diff(&baseline, &fresh);
    open_from_digests(
        &store,
        "r",
        "digest-diff",
        "",
        "x",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();
    assert!(resolve(&store, "r", "nonexistent", "ack", "", "a").is_err());
    assert!(resolve(&store, "r", "crates/x/src/core/store.rs", "bogus", "", "a").is_err());
    let ev = resolve(
        &store,
        "r",
        "crates/x/src/core/store.rs",
        "noop",
        "already routed",
        "agent",
    )
    .unwrap();
    let t = ev
        .targets
        .iter()
        .find(|t| t.path == "crates/x/src/core/store.rs")
        .unwrap();
    assert_eq!(t.status, "noop");
}

#[test]
fn archive_files_closed_events_and_keeps_open_promises() {
    let tmp = std::env::temp_dir().join(format!("harna-parity-arch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let (baseline, fresh) = digests();
    let d = diff::diff(&baseline, &fresh);
    // `closed`: every target resolved. `owed`: stays open.
    let mut closed = open_from_digests(
        &store,
        "closed",
        "digest-diff",
        "",
        "x",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();
    for t in closed.targets.clone() {
        closed = resolve(&store, "closed", &t.path, "noop", "nothing needed", "test").unwrap();
    }
    assert!(
        !closed.targets.is_empty(),
        "the closed event had targets to resolve"
    );
    open_from_digests(
        &store,
        "owed",
        "digest-diff",
        "",
        "x",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();

    let archived = archive_closed(&store).unwrap();
    assert_eq!(archived, vec!["closed".to_string()]);
    assert!(
        store.archive_dir().join("parity/closed.toml").is_file(),
        "the filed event keeps its bytes, out of the live surface"
    );
    let live = list_events(&store).unwrap();
    assert_eq!(
        live.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        vec!["owed"]
    );

    // Idempotent: a second pass finds nothing (open stays open).
    assert!(archive_closed(&store).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}

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

#[test]
fn bulk_resolve_applies_and_reports_missing() {
    let tmp = std::env::temp_dir().join(format!("harna-parity-bulk-{}", std::process::id()));
    // Windows reuses process ids: a stale dir from a previous run would
    // carry old parity events in.
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let (baseline, fresh) = digests();
    let d = diff::diff(&baseline, &fresh);
    let _event = open_from_digests(
        &store,
        "bulk-test",
        "digest-diff",
        "",
        "HEAD",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();

    let entries = vec![
        (
            "crates/x/src/core/store.rs".into(),
            "ack".into(),
            "routed through transiter".into(),
        ),
        (
            "crates/x/src/core/added.rs".into(),
            "noop".into(),
            "nothing needed".into(),
        ),
        ("crates/ghost.rs".into(), "noop".into(), String::new()),
    ];
    let (ev, missing) = resolve_bulk(&store, "bulk-test", &entries, "bulk test").unwrap();
    assert_eq!(missing, vec!["crates/ghost.rs".to_string()]);
    assert_eq!(
        ev.targets
            .iter()
            .find(|t| t.path == "crates/x/src/core/store.rs")
            .unwrap()
            .status,
        "ack"
    );
    assert_eq!(
        ev.targets
            .iter()
            .find(|t| t.path == "crates/x/src/core/added.rs")
            .unwrap()
            .status,
        "noop"
    );
    assert_eq!(ev.open_targets().len(), 1, "only the importer stays open");
    let rendered = ev.render();
    assert!(rendered.contains("resolved by bulk test"));
}

#[test]
fn debt_view_hides_resolved_and_auto() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-parity-openview-{}", std::process::id()));
    // Windows reuses process ids: a stale dir from a previous run would
    // carry old parity events in.
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let (baseline, fresh) = digests();
    let d = diff::diff(&baseline, &fresh);
    open_from_digests(
        &store,
        "debtview",
        "digest-diff",
        "",
        "HEAD",
        &baseline,
        &fresh,
        &d,
        &workspace(),
    )
    .unwrap();

    // Origins resolve themselves; the breaking importer is the debt.
    let text = show(&store, Some("debtview"), true).unwrap();
    assert!(text.contains("crates/y/src/api.rs"));
    assert!(
        !text.contains("crates/x/src/core/added.rs"),
        "origins are not debt"
    );
    assert!(
        !text.contains("origin"),
        "debt view lists only open targets"
    );

    resolve(
        &store,
        "debtview",
        "crates/y/src/api.rs",
        "noop",
        "only uses load_v2 already",
        "t",
    )
    .unwrap();
    assert_eq!(
        show(&store, Some("debtview"), true).unwrap(),
        "no open parity debts"
    );
    // Full view shows provenance, origin marks, and the stamp line.
    let full = show(&store, Some("debtview"), false).unwrap();
    assert!(full.contains("· origin"), "{full}");
    assert!(full.contains("noop"));
    assert!(
        full.contains("hand-resolved: 1 · rubber-stamp signals 0 (0%)"),
        "{full}"
    );
    Ok(())
}

#[test]
fn crate_rel_resolves_workspace_paths() {
    assert_eq!(
        crate_rel("crates/harna-core/src/schema/store.rs").as_deref(),
        Some("schema/store")
    );
    assert_eq!(crate_rel("src/main.rs").as_deref(), Some("main"));
    assert_eq!(
        crate_rel("crates/x/src/engine/billing/mod.rs").as_deref(),
        Some("engine/billing")
    );
}

#[test]
fn super_and_self_edges_resolve_to_importer_context() {
    use crate::search::digest::{resolve_dep, DepTarget};

    // super::x from module a/b reaches a/x
    assert_eq!(
        resolve_dep("super::x", "a/b", &[]),
        Some(DepTarget {
            external: None,
            stem: "a/x".into()
        })
    );
    // bare super:: from a/b is the parent module itself
    assert_eq!(
        resolve_dep("super::", "a/b", &[]),
        Some(DepTarget {
            external: None,
            stem: "a".into()
        })
    );
    // self::x from a/b stays inside a/b
    assert_eq!(
        resolve_dep("self::x", "a/b", &[]),
        Some(DepTarget {
            external: None,
            stem: "a/b/x".into()
        })
    );
    // crate-root files cannot use super::
    assert_eq!(resolve_dep("super::x", "main", &[]), None);

    // And the fan-out matcher honors it: importer a/b.rs depends on
    // super::x; module a/x.rs is a target.
    assert!(dep_matches("a/b", "super::x", "crates/t/src/a/x.rs", &[]));
    assert!(!dep_matches(
        "a/b",
        "super::x",
        "crates/t/src/other/x.rs",
        &[]
    ));
}

#[test]
fn shape_orientation_names_template_and_ceiling() {
    use crate::schema::registers::parity_templates::{ParityTemplate, ParityTemplates};

    let tmp = std::env::temp_dir().join(format!(
        "harna-parity-shape-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();

    let event = ParityEvent {
        id: "big-shape".into(),
        kind: "digest-diff".into(),
        source: "fixture".into(),
        generation: 1,
        baseline_ref: "HEAD".into(),
        created: "t".into(),
        updated: "t".into(),
        targets: vec![
            ParityTarget {
                path: "crates/x/src/schema/store.rs".into(),
                reason: "changed".into(),
                status: "open".into(),
                note: String::new(),
                by: "t".into(),
                when: String::new(),
                breaking: false,
            },
            ParityTarget {
                path: "crates/x/src/schema/add.rs".into(),
                reason: "added".into(),
                status: "open".into(),
                note: String::new(),
                by: "t".into(),
                when: String::new(),
                breaking: false,
            },
            ParityTarget {
                path: "crates/x/src/schema/drop.rs".into(),
                reason: "added".into(),
                status: "open".into(),
                note: String::new(),
                by: "t".into(),
                when: String::new(),
                breaking: false,
            },
            ParityTarget {
                path: "crates/x/src/schema/fix.rs".into(),
                reason: "added".into(),
                status: "open".into(),
                note: String::new(),
                by: "t".into(),
                when: String::new(),
                breaking: false,
            },
            ParityTarget {
                path: "crates/x/src/core/other.rs".into(),
                reason: "changed".into(),
                status: "open".into(),
                note: String::new(),
                by: "t".into(),
                when: String::new(),
                breaking: false,
            },
        ],
    };
    save_event(&store, &event).unwrap();

    // Only the schema-scope template matches any of the event's targets.
    store
        .save_templates(&ParityTemplates {
            schema_version: crate::SCHEMA_VERSION,
            templates: vec![
                ParityTemplate {
                    id: "schema-change".into(),
                    scope: "crates/x/src/schema".into(),
                    max_targets: 2,
                    rationale: "shard edits stay small".into(),
                },
                ParityTemplate {
                    id: "engine-change".into(),
                    scope: "crates/x/src/engine".into(),
                    max_targets: 1,
                    rationale: "no match".into(),
                },
            ],
        })
        .unwrap();
    let mut out = Vec::new();
    check_shape(&store, &mut out);
    assert_eq!(out.len(), 1, "one template's ceiling was walked past");
    let v = &out[0];
    assert_eq!(v.check, "fanout-shape");
    assert_eq!(v.severity, Severity::Info);
    assert!(v.message.contains("big-shape"));
    assert!(v.message.contains("schema-change"), "names the template");
    assert!(
        v.message.contains("4 target(s)") || v.message.contains("carries 4"),
        "the actual shape"
    );
    assert!(v.message.contains("at most 2"), "the declared shape");

    // A raised ceiling (or deleted templates) means nothing to notice.
    store
        .save_templates(&ParityTemplates {
            schema_version: crate::SCHEMA_VERSION,
            templates: vec![ParityTemplate {
                id: "schema-change".into(),
                scope: "crates/x/src/schema".into(),
                max_targets: 50,
                rationale: "raised".into(),
            }],
        })
        .unwrap();
    let mut out = Vec::new();
    check_shape(&store, &mut out);
    assert!(out.is_empty());

    let _ = std::fs::remove_dir_all(&tmp);
}
