use super::*;

#[test]
fn normalize_root_lexicalizes_and_prefixes_long_paths() {
    let v = format!("{}{}?{}", '\\', '\\', '\\'); // the verbatim head, backslash backslash question backslash

    // Short existing path: canonicalized, stays plain (no verbatim prefix).
    assert!(!normalize_root(Path::new(".")).to_string_lossy().starts_with(&v));

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
    let mut deep = String::from(if cfg!(windows) { format!("C:{}deep", '\\') } else { "/deep".into() });
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
    assert_eq!(store.load_non_goals()?, g, "decline register is byte-faithful");

    let mut l = crate::schema::registers::lessons::Lessons::default();
    l.upsert(crate::schema::registers::lessons::Lesson {
        id: "ps-roundtrip".into(),
        claim: "bulk-edit sources via PowerShell".into(),
        verdict: "encoding corruption".into(),
        evidence: vec!["commit d4e81ed".into()],
        recorded: "2026-10-17".into(),
    });
    store.save_lessons(&l)?;
    assert_eq!(store.load_lessons()?, l, "dead-ends register is byte-faithful");
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

    let mut a = crate::schema::registers::acknowledged::Acknowledged::default();
    a.upsert(crate::schema::registers::acknowledged::Acknowledgement {
        check: "hotspot".into(),
        file: "big.rs".into(),
        reason: "tracked elsewhere".into(),
        recorded: "2026-10-17".into(),
    });
    store.save_acknowledged(&a)?;
    assert_eq!(store.load_acknowledged()?, a, "save -> load is byte-faithful");
    std::fs::remove_dir_all(tmp.join(".harna"))?;
    Ok(())
}

#[test]
fn same_day_same_title_decisions_never_overwrite() -> anyhow::Result<()> {
    let tmp =
        std::env::temp_dir().join(format!("harna-decision-collide-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    let store = Store::open(&tmp)?;
    let first = store.save_decision("Refactor", "first record", None)?;
    let second = store.save_decision("Refactor", "second record", None)?;
    assert_ne!(first, second);
    assert!(second.to_string_lossy().ends_with("-refactor_1.md"), "{second:?}");
    assert!(std::fs::read_to_string(&first)?.contains("first record"));
    assert!(std::fs::read_to_string(&second)?.contains("second record"));
    std::fs::remove_dir_all(tmp.join(".harna"))?;
    Ok(())
}

#[test]
fn save_decision_records_the_supersedes_pointer() -> anyhow::Result<()> {
    let tmp =
        std::env::temp_dir().join(format!("harna-decision-store-{}", std::process::id()));
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

fn shard_manifest() -> Manifest {
    use crate::schema::model::manifest::{Container, Layer};

    let container = |name: &str, path: &str| Container {
        name: name.into(),
        path: path.into(),
        ..Default::default()
    };
    Manifest {
        schema_version: crate::SCHEMA_VERSION,
        layers: vec![
            Layer {
                name: "l1".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                  role: None,
                containers: vec![
                    container("one", "crates/t/src/one"),
                    container("two", "crates/t/src/two"),
                ],
            },
            Layer {
                name: "l2".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                containers: vec![container("uno", "crates/t/src/one/nested")],
                   role: None,
            },
        ],
    }
}

fn shard_module(path: &str, hash: &str) -> Module {
    Module {
        path: path.into(),
        lang: "rust".into(),
        signatures: vec!["pub fn f()".into()],
        deps: vec![],
        body_hash: hash.into(),
        inventory: vec![],
        mods: vec![],
    }
}

fn shard_digest() -> Digest {
    Digest {
        generated_at: "then".into(),
        extractor_version: crate::schema::map::EXTRACTOR_VERSION,
        modules: vec![
            shard_module("crates/t/src/one/a.rs", "ha"),
            // Nested container wins by longest prefix.
            shard_module("crates/t/src/one/nested/deep.rs", "hd"),
            shard_module("crates/t/src/two/b.rs", "hb"),
            shard_module("crates/t/src/stray.rs", "hs"),
        ],
    }
}

#[test]
fn removed_containers_leave_no_orphaned_shards() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "harna-shards-orphan-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    store.save_digest(&shard_manifest(), &shard_digest())?;
    let shards = store.digest_dir().join("shards");
    assert!(shards.join("l1-two.json").is_file());
    // Container `two` removed from the manifest: its modules go unhomed.
    let mut m = shard_manifest();
    m.layers[0].containers.retain(|c| c.name != "two");
    store.save_digest(&m, &shard_digest())?;
    assert!(!shards.join("l1-two.json").exists(), "orphan shard removed");
    assert!(shards.join("l1-one.json").is_file(), "live shards kept");
    let listed = store.load_index()?.shards.len();
    let on_disk = std::fs::read_dir(&shards)?.flatten().count();
    assert_eq!(on_disk, listed, "disk mirrors the index");
    assert_eq!(store.load_digest()?.modules.len(), 4, "nothing lost");
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[test]
fn digest_shards_per_container_and_round_trips_identical() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "harna-shards-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    let m = shard_manifest();
    let d = shard_digest();
    store.save_digest(&m, &d)?;

    // One shard per home (nested longest-prefix wins) plus _unhomed.
    let dir = store.digest_dir();
    assert!(dir.join("index.json").is_file());
    assert!(dir.join("shards/l1-one.json").is_file());
    assert!(dir.join("shards/l2-uno.json").is_file());
    assert!(dir.join("shards/l1-two.json").is_file());
    assert!(dir.join("shards/unhomed.json").is_file(), "slug trims to `unhomed`");

    // The index carries stamps plus compact per-module records.
    let raw = std::fs::read_to_string(dir.join("index.json"))?;
    let index: DigestIndex = serde_json::from_str(&raw)?;
    assert_eq!(index.generated_at, "then");
    assert_eq!(index.shards.len(), 4);
    let one = index.shards.iter().find(|s| s.container == "one").unwrap();
    assert_eq!(one.layer, "l1");
    assert_eq!(one.modules.len(), 1);
    assert_eq!(one.modules[0].body_hash, "ha", "reuse hash rides the index");

    // Module-for-module identical, order preserved.
    let back = store.load_digest()?;
    assert_eq!(back.generated_at, "then");
    assert_eq!(
        back.modules.iter().map(|x| x.path.clone()).collect::<Vec<_>>(),
        d.modules.iter().map(|x| x.path.clone()).collect::<Vec<_>>()
    );
    assert_eq!(back.modules[0].body_hash, "ha");
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[test]
fn legacy_monolith_loads_until_the_next_scan_migrates() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "harna-shards-legacy-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    store.init()?;
    // A pre-0.3.15 repo: monolith present, no shards.
    let d = shard_digest();
    std::fs::write(store.digest_path(), serde_json::to_string_pretty(&d)?)?;
    let back = store.load_digest()?;
    assert_eq!(back.modules.len(), 4, "legacy fallback reads the monolith");
    // The next scan migrates forward and removes the monolith.
    store.save_digest(&shard_manifest(), &d)?;
    assert!(!store.digest_path().exists(), "one truth, never two");
    assert_eq!(store.load_digest()?.modules.len(), 4);
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[test]
fn empty_digest_writes_an_empty_index() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "harna-shards-empty-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    let d = Digest {
        generated_at: "then".into(),
        extractor_version: crate::schema::map::EXTRACTOR_VERSION,
        modules: vec![],
    };
    store.save_digest(&shard_manifest(), &d)?;
    let back = store.load_digest()?;
    assert!(back.modules.is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[test]
fn bounded_stream_visits_in_digest_order_with_stamps_and_stops_early() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "harna-bounded-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    store.save_digest(&shard_manifest(), &shard_digest())?;

    // Full stream: digest order across shards, stamps from the index.
    let mut seen: Vec<String> = Vec::new();
    let mut stamps = Vec::new();
    store.for_each_shard(|stamp, shard| {
        stamps.push((stamp.generated_at.clone(), stamp.extractor_version));
        for m in &shard.modules {
            seen.push(m.path.clone());
        }
        true
    })?;
    assert_eq!(
        seen,
        vec![
            "crates/t/src/one/a.rs",
            "crates/t/src/one/nested/deep.rs",
            "crates/t/src/two/b.rs",
            "crates/t/src/stray.rs",
        ]
    );
    assert!(stamps.iter().all(|(at, _)| at == "then"));
    assert!(stamps.iter().all(|(_, v)| *v == crate::schema::map::EXTRACTOR_VERSION));

    // Early stop: the visitor's false ends the stream after one module.
    let mut seen: Vec<String> = Vec::new();
    store.for_each_shard(|_, shard| {
        match shard.modules.first() {
            Some(m) => {
                seen.push(m.path.clone());
                false
            }
            None => true,
        }
    })?;
    assert_eq!(seen.len(), 1);
    let _ = std::fs::remove_dir_all(&tmp);
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
    assert!(home.load_rules()?.rules.is_empty(), "the starter rules survive a refused profile");
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
    assert_eq!(std::fs::read_to_string(&dest)?, "x = 1", "first archive intact");
    assert_eq!(std::fs::read_to_string(&second)?, "x = 2");
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[test]
fn bounded_stream_is_bilingual_and_reports_missing_maps() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "harna-bounded-legacy-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    store.init()?;

    // No map at all: Ok(false), the caller owns the scan refusal.
    assert!(!store.for_each_shard(|_, _| true)?);

    // A pre-0.3.15 repo: the monolith streams as a single pass and its
    // stamps ride along the same way the index's do.
    std::fs::write(store.digest_path(), serde_json::to_string_pretty(&shard_digest())?)?;
    let mut passes = 0usize;
    let mut seen: Vec<String> = Vec::new();
    store.for_each_shard(|stamp, shard| {
        passes += 1;
        assert_eq!(stamp.generated_at, "then");
        for m in &shard.modules {
            seen.push(m.path.clone());
        }
        true
    })?;
    assert_eq!(passes, 1);
    assert_eq!(seen.len(), 4);
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
    assert!(raw.starts_with("schema_version = 1\n\n# Architecture notes a human wrote.\n# Second line.\n"), "{raw}");
    assert_eq!(store.load_manifest().unwrap().layers.len(), 1, "still valid data");
    store.save_manifest(&store.load_manifest().unwrap()).unwrap();
    assert_eq!(std::fs::read_to_string(store.manifest_path()).unwrap(), raw, "stable across saves");
    let _ = std::fs::remove_dir_all(&tmp);
}
