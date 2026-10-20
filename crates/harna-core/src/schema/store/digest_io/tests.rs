//! The sharded map on disk: shards per container, round trips, legacy
//! migration, streamed reads, orphan cleanup.

use super::*;

fn shard_manifest() -> Manifest {
    use crate::schema::model::manifest::{Container, Layer};

    let container = |name: &str, path: &str| Container {
        name: name.into(),
        path: path.into(),
        ..Default::default()
    };
    Manifest {
        schema_version: crate::SCHEMA_BASELINE,
        layers: vec![
            Layer {
                owners: Vec::new(),
                isolated: false,
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
                owners: Vec::new(),
                isolated: false,
                name: "l2".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                containers: vec![container("uno", "crates/t/src/one/nested")],
                role: None,
            },
        ],
        ..Default::default()
    }
}

fn shard_module(path: &str, hash: &str) -> Module {
    Module {
        path: path.into(),
        lang: "rust".into(),
        signatures: vec!["pub fn f()".into()],
        body_hash: hash.into(),
        ..Default::default()
    }
}

fn shard_digest() -> Digest {
    Digest {
        orphans: Vec::new(),
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
    assert!(
        dir.join("shards/unhomed.json").is_file(),
        "slug trims to `unhomed`"
    );

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
        back.modules
            .iter()
            .map(|x| x.path.clone())
            .collect::<Vec<_>>(),
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
        orphans: Vec::new(),
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
    assert!(stamps
        .iter()
        .all(|(_, v)| *v == crate::schema::map::EXTRACTOR_VERSION));

    // Early stop: the visitor's false ends the stream after one module.
    let mut seen: Vec<String> = Vec::new();
    store.for_each_shard(|_, shard| match shard.modules.first() {
        Some(m) => {
            seen.push(m.path.clone());
            false
        }
        None => true,
    })?;
    assert_eq!(seen.len(), 1);
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
    std::fs::write(
        store.digest_path(),
        serde_json::to_string_pretty(&shard_digest())?,
    )?;
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

/// 0.9.5: the same content with CRLF line endings is not rewritten.
#[test]
fn an_unchanged_map_file_is_not_rewritten_whatever_its_line_endings() {
    let dir = std::env::temp_dir().join(format!("harna-eol-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("shard.json");
    std::fs::write(&p, "{\r\n  \"a\": 1\r\n}").unwrap();
    write_if_changed(&p, "{\n  \"a\": 1\n}".to_string()).unwrap();
    assert!(
        std::fs::read(&p).unwrap().contains(&b'\r'),
        "kept as it was"
    );
    write_if_changed(&p, "{\n  \"a\": 2\n}".to_string()).unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\n  \"a\": 2\n}");
    let _ = std::fs::remove_dir_all(&dir);
}
