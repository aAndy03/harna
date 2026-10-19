use super::*;

#[test]
fn normalize_root_lexicalizes_and_prefixes_long_paths() {
    let v = format!("{}{}?{}", '\\', '\\', '\\'); // the verbatim head, backslash backslash question backslash

    // Short existing path: canonicalized, stays plain (no verbatim prefix).
    assert!(!normalize_root(Path::new("."))
        .to_string_lossy()
        .starts_with(&v));

    // Lexical fallback for a NON-existing path: dots resolved, separators
    // joined to the path's own style, head never popped.
    let mixed = format!("some{}repo/sub/../deep", '\\');
    assert_eq!(
        normalize_root(Path::new(&mixed)).to_string_lossy(),
        "some/repo/deep",
        "mixed separators normalize onto the |/| style"
    );
    let stray = format!("..{}keep", '\\');
    assert_eq!(normalize_root(Path::new(&stray)).to_string_lossy(), stray);

    // A 60-segment path passes the prefix mark on Windows and rides the
    // verbatim form; short paths never grow one.
    let mut deep = if cfg!(windows) {
        format!("C:{}deep", '\\')
    } else {
        "/deep".into()
    };
    for i in 0..60 {
        deep.push_str(&format!("{}segment-segment-{:02}", '\\', i));
    }
    let normalized = normalize_root(Path::new(&deep));
    if cfg!(windows) {
        assert!(
            normalized.to_string_lossy().starts_with(&v),
            "long paths ride the verbatim form: {:?}",
            normalized
        );
    } else {
        assert!(normalized.to_string_lossy().starts_with("/deep"));
    }
    assert_eq!(
        normalize_root(Path::new("sub/here")).to_string_lossy(),
        "sub/here"
    );
}

#[test]
fn init_is_idempotent_and_loads_defaults() {
    let tmp = std::env::temp_dir().join(format!("harna-test-{}", std::process::id()));
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    store.init().unwrap(); // second call must not fail

    let m = store.load_manifest().unwrap();
    assert!(m.is_empty());
    let r = store.load_rules().unwrap();
    assert!(r.is_empty());
}

#[test]
fn the_glossary_round_trips_and_defaults_to_empty() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-terms-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    let store = Store::open(&tmp)?;
    store.init()?;

    // No glossary yet: the empty state, not an error.
    assert!(store.load_terms()?.is_empty());

    let mut t = crate::schema::registers::terms::Terms::default();
    t.terms.push(crate::schema::registers::terms::Term {
        word: "engine".into(),
        aliases: vec!["service".into()],
        means: "owns a concern end-to-end".into(),
    });
    store.save_terms(&t)?;
    let back = store.load_terms()?;
    assert_eq!(back, t, "save -> load is byte-faithful");
    std::fs::remove_dir_all(tmp.join(".harna"))?;
    Ok(())
}

#[test]
fn decline_and_deadend_registers_round_trip_and_default_to_empty() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-registers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    let store = Store::open(&tmp)?;
    store.init()?;

    // Absent registers: the empty state, not an error.
    assert!(store.load_non_goals()?.is_empty());
    assert!(store.load_lessons()?.is_empty());

    let mut g = crate::schema::registers::non_goals::NonGoals::default();
    g.upsert(crate::schema::registers::non_goals::NonGoal {
        id: "no-remote-state".into(),
        refusal: "harna does not probe live production systems".into(),
        reason: "production awareness is a read-only surface".into(),
        recorded: "2026-10-17".into(),
    });
    store.save_non_goals(&g)?;
    assert_eq!(
        store.load_non_goals()?,
        g,
        "decline register is byte-faithful"
    );

    let mut l = crate::schema::registers::lessons::Lessons::default();
    l.upsert(crate::schema::registers::lessons::Lesson {
        id: "ps-roundtrip".into(),
        claim: "bulk-edit sources via PowerShell".into(),
        verdict: "encoding corruption".into(),
        evidence: vec!["commit d4e81ed".into()],
        recorded: "2026-10-17".into(),
    });
    store.save_lessons(&l)?;
    assert_eq!(
        store.load_lessons()?,
        l,
        "dead-ends register is byte-faithful"
    );
    std::fs::remove_dir_all(tmp.join(".harna"))?;
    Ok(())
}

#[test]
fn the_roadmap_round_trips_and_defaults_to_empty() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-roadmap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    let store = Store::open(&tmp)?;
    store.init()?;

    // No roadmap yet: the empty state, not an error.
    assert!(store.load_roadmap()?.is_empty());

    let mut r = crate::schema::registers::roadmap::Roadmap::default();
    r.upsert(crate::schema::registers::roadmap::Milestone {
        id: "0.3.28".into(),
        title: "roadmap as data".into(),
        state: "current".into(),
        notes: "declared, never generated".into(),
    });
    store.save_roadmap(&r)?;
    assert_eq!(store.load_roadmap()?, r, "save -> load is byte-faithful");
    std::fs::remove_dir_all(tmp.join(".harna"))?;
    Ok(())
}

#[test]
fn the_advisory_ledger_round_trips_and_defaults_to_empty() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-ack-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    let store = Store::open(&tmp)?;
    store.init()?;

    // Nothing acknowledged: the empty state, not an error.
    assert!(store.load_acknowledged()?.is_empty());

    let mut a = crate::schema::registers::ledgers::acknowledged::Acknowledged::default();
    a.upsert(
        crate::schema::registers::ledgers::acknowledged::Acknowledgement {
            check: "hotspot".into(),
            file: "big.rs".into(),
            reason: "tracked elsewhere".into(),
            recorded: "2026-10-17".into(),
        },
    );
    store.save_acknowledged(&a)?;
    assert_eq!(
        store.load_acknowledged()?,
        a,
        "save -> load is byte-faithful"
    );
    std::fs::remove_dir_all(tmp.join(".harna"))?;
    Ok(())
}

#[test]
fn same_day_same_title_decisions_never_overwrite() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-decision-collide-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    let store = Store::open(&tmp)?;
    let first = store.save_decision("Refactor", "first record", None)?;
    let second = store.save_decision("Refactor", "second record", None)?;
    assert_ne!(first, second);
    assert!(
        second.to_string_lossy().ends_with("-refactor_1.md"),
        "{second:?}"
    );
    assert!(std::fs::read_to_string(&first)?.contains("first record"));
    assert!(std::fs::read_to_string(&second)?.contains("second record"));
    std::fs::remove_dir_all(tmp.join(".harna"))?;
    Ok(())
}

#[test]
fn save_decision_records_the_supersedes_pointer() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-decision-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    let store = Store::open(&tmp).unwrap();
    let path = store.save_decision("New plan", "replaces the old one", Some("old.md"))?;
    let written = std::fs::read_to_string(path)?;
    assert!(written.starts_with("# New plan"));
    assert!(written.contains("supersedes: old.md"));

    // Persists visibly: the decision-log parser reads it back through
    // the same store.
    let decisions = crate::schema::journal::decisions::list(&store.decisions_dir())?;
    assert!(decisions
        .iter()
        .any(|d| d.supersedes.as_deref() == Some("old.md")));
    std::fs::remove_dir_all(tmp.join(".harna"))?;
    Ok(())
}

#[test]
fn profile_copies_rules_and_manifest_verbatim() -> anyhow::Result<()> {
    // A source repo's .harna with a distinctive rule + layer; init
    // --from copies them byte-honest so the adopter prunes real data.
    let src = std::env::temp_dir().join(format!(
        "harna-profile-src-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let dst = std::env::temp_dir().join(format!(
        "harna-profile-dst-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = (std::fs::remove_dir_all(&src), std::fs::remove_dir_all(&dst));
    std::fs::create_dir_all(src.join(".harna"))?;
    std::fs::write(
        src.join(".harna/rules.toml"),
        "[[rules]]\nid = \"src-rule\"\ntext = \"t\"\nrationale = \"why\"\norigin = \"profile test\"\n",
    )?;
    std::fs::write(
        src.join(".harna/manifest.toml"),
        "[[layers]]\nname = \"transport\"\n\n[[layers.containers]]\nname = \"wire\"\npath = \"src\"\n",
    )?;
    let home = Store::open(&dst)?;
    home.init()?;
    let applied = home.apply_profile(&src)?;

    assert!(applied.manifest && applied.rules);
    assert_eq!(home.load_rules()?.rules[0].id, "src-rule");
    let m = home.load_manifest()?;
    assert_eq!(m.layers[0].name, "transport");
    assert_eq!(m.layers[0].containers[0].name, "wire");
    let _ = (std::fs::remove_dir_all(&src), std::fs::remove_dir_all(&dst));
    Ok(())
}

#[test]
fn profile_tolerates_missing_pieces_refuses_garbage() -> anyhow::Result<()> {
    let src = std::env::temp_dir().join(format!(
        "harna-profile-bad-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let dst = std::env::temp_dir().join(format!(
        "harna-profile-bad-dst-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = (std::fs::remove_dir_all(&src), std::fs::remove_dir_all(&dst));

    // Nothing there at all: clear refusal.
    let home = Store::open(&dst)?;
    home.init()?;
    assert!(home.apply_profile(&src).is_err());

    // Garbage manifest: refused BEFORE any overwrite — the starter
    // rules stay intact (a broken profile must not poison .harna/).
    std::fs::create_dir_all(src.join(".harna"))?;
    std::fs::write(src.join(".harna/manifest.toml"), "not [ valid toml {{{")?;
    let applied = home.apply_profile(&src);
    assert!(applied.is_err());
    assert!(
        home.load_rules()?.rules.is_empty(),
        "the starter rules survive a refused profile"
    );
    let _ = (std::fs::remove_dir_all(&src), std::fs::remove_dir_all(&dst));
    Ok(())
}

#[test]
fn archive_file_renames_the_bytes_out_of_the_live_surface() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "harna-archive-file-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    store.init()?;
    let src = store.scores_dir().join("demo.toml");
    std::fs::create_dir_all(store.scores_dir())?;
    std::fs::write(&src, "x = 1")?;

    let dest = store.archive_file(&src, "scores")?;
    assert!(dest.ends_with("archive/scores/demo.toml"));
    assert!(!src.exists(), "the live surface is lean");
    let filed = std::fs::read_to_string(&dest)?;
    assert_eq!(filed, "x = 1", "a rename: the bytes never change");
    // Idempotent: archiving a missing source is a no-op returning the path.
    let again = store.archive_file(&src, "scores")?;
    assert!(again.ends_with("archive/scores/demo.toml"));
    // A second live file with the same name never clobbers the first
    // archive: it takes the next `_N`.
    std::fs::write(&src, "x = 2")?;
    let second = store.archive_file(&src, "scores")?;
    assert!(second.ends_with("archive/scores/demo_1.toml"), "{second:?}");
    assert_eq!(
        std::fs::read_to_string(&dest)?,
        "x = 1",
        "first archive intact"
    );
    assert_eq!(std::fs::read_to_string(&second)?, "x = 2");
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[test]
fn a_typed_manifest_save_keeps_the_human_header() {
    let tmp = std::env::temp_dir().join(format!("harna-header-{:?}", std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    std::fs::write(
        store.manifest_path(),
        "schema_version = 1\n\n# Architecture notes a human wrote.\n# Second line.\n\n[[layers]]\nname = \"l\"\n",
    )
    .unwrap();
    let m = store.load_manifest().unwrap();
    store.save_manifest(&m).unwrap();
    let raw = std::fs::read_to_string(store.manifest_path()).unwrap();
    assert!(
        raw.starts_with(
            "schema_version = 1\n\n# Architecture notes a human wrote.\n# Second line.\n"
        ),
        "{raw}"
    );
    assert_eq!(
        store.load_manifest().unwrap().layers.len(),
        1,
        "still valid data"
    );
    store
        .save_manifest(&store.load_manifest().unwrap())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(store.manifest_path()).unwrap(),
        raw,
        "stable across saves"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 0.3.47 (H10): the orphans a scan records survive the sharded store —
/// they live in the index, beside the shards, never inside one.
#[test]
fn digest_orphans_round_trip_through_the_sharded_store() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-orphans-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    let store = Store::open(&tmp)?;
    store.init()?;
    let d = crate::search::digest::Digest {
        generated_at: "2026-10-19T00:00:00Z".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: Vec::new(),
        orphans: vec!["crates/a/src/evals/fixtures/before.rs".into()],
    };
    store.save_digest(&Default::default(), &d)?;
    assert_eq!(store.load_digest()?.orphans, d.orphans);
    std::fs::remove_dir_all(tmp.join(".harna"))?;
    Ok(())
}
