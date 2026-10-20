use super::*;
use crate::schema::model::manifest::{Component, ComponentKind, Container, Layer};

#[test]
fn grades_break_at_the_documented_bounds() {
    assert_eq!(grade_for(100), "A");
    assert_eq!(grade_for(90), "A");
    assert_eq!(grade_for(89), "B");
    assert_eq!(grade_for(80), "B");
    assert_eq!(grade_for(79), "C");
    assert_eq!(grade_for(70), "C");
    assert_eq!(grade_for(69), "D");
    assert_eq!(grade_for(60), "D");
    assert_eq!(grade_for(59), "F");
    assert_eq!(grade_for(0), "F");
}

#[test]
fn freshness_buckets_and_missing_map() {
    assert_eq!(freshness_points(Some(0)), 15);
    assert_eq!(freshness_points(Some(299)), 15);
    assert_eq!(
        freshness_points(Some(300)),
        12,
        "boundary is exclusive below"
    );
    assert_eq!(freshness_points(Some(3_599)), 12);
    assert_eq!(freshness_points(Some(3_600)), 8);
    assert_eq!(freshness_points(Some(86_399)), 8);
    assert_eq!(freshness_points(Some(86_400)), 4);
    assert_eq!(freshness_points(Some(604_799)), 4);
    assert_eq!(freshness_points(Some(604_800)), 0);
    assert_eq!(freshness_points(Some(9_999_999)), 0);
    assert_eq!(freshness_points(None), 0, "no digest, no freshness");
}

#[test]
fn perfect_inputs_grade_a_and_costs_floor_at_zero() {
    let s = assemble(100, 0, 0, Some(10), 0, 0);
    assert_eq!((s.score, s.grade.as_str()), (100, "A"));
    assert_eq!(
        s.parts,
        ScoreParts {
            coverage: 30,
            debts: 20,
            freshness: 15,
            boundaries: 15,
            coupling: 20
        }
    );
    // Debts and outliers saturate instead of underflowing.
    let s = assemble(0, 999, 0, None, 100, 999);
    assert_eq!((s.parts.debts, s.parts.coupling), (0, 0));
    assert_eq!((s.score, s.grade.as_str()), (0, "F"));
}

#[test]
fn overdue_deferred_reviews_cost_like_parity_debts() {
    // Two overdue reviews, no parity debt: each costs DEBT_COST.
    let s = assemble(100, 0, 2, Some(10), 0, 0);
    assert_eq!(s.parts.debts, WEIGHT_DEBTS - 2 * DEBT_COST);
    // Deferred alone saturates the debts part, never underflows it.
    let s = assemble(100, 0, 999, Some(10), 0, 0);
    assert_eq!(s.parts.debts, 0);
    // Sorting the register stops the bleeding (0.3.23's point).
    let s = assemble(100, 3, 0, Some(10), 0, 0);
    assert_eq!(s.parts.debts, WEIGHT_DEBTS - 3 * DEBT_COST);
}

#[test]
fn coverage_floors_through_the_assembly() {
    // 2/3 → 66 → 66*30/100 = 19 (floored twice, never flattered).
    let s = assemble(66, 0, 0, Some(10), 0, 0);
    assert_eq!(s.parts.coverage, 19);
    assert_eq!(s.score, 19 + 20 + 15 + 15 + 20);
}

#[test]
fn slugs_are_filesystem_safe_and_collisions_suffix() -> anyhow::Result<()> {
    assert_eq!(
        snapshot_slug("2026-10-16T12:34:56.789+00:00"),
        "2026-10-16t12-34-56-789-00-00"
    );
    assert_eq!(snapshot_slug(""), "");
    let tmp = std::env::temp_dir().join(format!(
        "harna-snaps-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    let at = "2026-10-16T12:00:00+00:00";
    let mut a = assemble(100, 0, 0, Some(10), 0, 0);
    a.computed_at = at.into();
    let p1 = record(&store, &a)?;
    let p2 = record(&store, &a)?;
    assert!(p1.ends_with("2026-10-16t12-00-00-00-00.toml"));
    assert!(
        p2.ends_with("2026-10-16t12-00-00-00-00_1.toml"),
        "append, never replace"
    );
    let snaps = list_snapshots(&store)?;
    assert_eq!(snaps.len(), 2);
    assert!(snaps[0].name < snaps[1].name, "oldest first");
    assert_eq!(snaps[1].score.score, 100);
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[test]
fn trend_flags_regressions_at_the_threshold() {
    let mk = |name: &str, score: u8| {
        let mut s = assemble(100, 0, 0, Some(10), 0, 0);
        s.score = score;
        Snapshot {
            name: name.into(),
            score: s,
        }
    };
    let snaps = vec![mk("t1", 80), mk("t2", 76), mk("t3", 71), mk("t4", 77)];
    let rows = trend(&snaps);
    assert_eq!(rows[0].delta, None, "first recording has no delta");
    assert!(!rows[0].regression);
    assert_eq!(rows[1].delta, Some(-4));
    assert!(!rows[1].regression, "-4 drifts, does not flag");
    assert_eq!(rows[2].delta, Some(-5));
    assert!(rows[2].regression, "-5 is a regression");
    assert_eq!(rows[3].delta, Some(6));
    assert!(!rows[3].regression, "recoveries never flag");
    let text = render_trend(&rows);
    assert!(text.contains("▼ REGRESSION"));
    assert!(render_trend(&[]).contains("no snapshots yet"));
}

#[test]
fn trend_attributes_moves_to_parts() {
    let snap = |name: &str, coverage_pct: u8, open: usize| {
        let s = assemble(coverage_pct, open, 0, Some(10), 0, 0);
        Snapshot {
            name: name.into(),
            score: s,
        }
    };
    // a: 30+20+15+15+20 = 100; b: 15+12+15+15+20 = 77.
    let snaps = vec![snap("a", 100, 0), snap("b", 50, 2)];
    let rows = trend(&snaps);
    assert!(
        rows[0].moved.is_empty(),
        "first recording attributes nothing"
    );
    assert_eq!(rows[1].delta, Some(-23));
    assert_eq!(
        rows[1].moved,
        vec!["coverage -15".to_string(), "debts -8".to_string()]
    );
    assert!(rows[1].regression);
    assert!(render_trend(&rows).contains("(coverage -15, debts -8)"));
}

#[test]
fn full_report_is_consistent_across_its_three_consumers() -> anyhow::Result<()> {
    // One scan, many consumers (0.3.17): metrics, score and hotspots
    // measure the same map read — the numbers must agree with what each
    // consumer produced standalone in the whole-map era.
    let tmp = std::env::temp_dir().join(format!(
        "harna-score-full-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    store.init()?;
    let manifest = crate::schema::model::manifest::Manifest {
        schema_version: 1,
        layers: vec![crate::schema::model::manifest::Layer {
            isolated: false,
            name: "l".into(),
            description: String::new(),
            sequence: vec![],
            depends_on: vec![],
            role: None,
            containers: vec![crate::schema::model::manifest::Container {
                escapes: Vec::new(),
                owns: Vec::new(),
                name: "c".into(),
                path: "src".into(),
                description: String::new(),
                sequence: vec![],
                provides: vec![],
                role: None,
                gateway: None,
                connects: vec![],
                components: vec![],
            }],
        }],
    };
    store.save_manifest(&manifest)?;
    let mk = |path: &str| crate::search::digest::Module {
        reexports: Vec::new(),
        crate_dir: None,
        src_root: None,
        path: path.into(),
        lang: "rust".into(),
        signatures: vec![],
        deps: vec![],
        body_hash: "x".into(),
        inventory: vec![],
        mods: vec![],
    };
    store.save_digest(
        &manifest,
        &crate::search::digest::Digest {
            orphans: Vec::new(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![mk("src/a.rs"), mk("src/b.rs")],
        },
    )?;

    let r = full(&store)?;
    // Metrics: two modules, no edges, no pressure.
    assert_eq!(r.metrics.modules.len(), 2);
    assert_eq!(
        (r.metrics.total_edges, r.metrics.boundary_pressure_pct),
        (0, 0)
    );
    // Score: no coverage (no components) + fresh map = debts+fresh+boundaries+coupling.
    assert_eq!(r.score.parity_open, 0);
    assert_eq!(r.score.boundary_pressure_pct, 0);
    assert_eq!(r.score.outlier_count, 0);
    assert_eq!(r.score.parts.freshness, 15, "fresh map");
    // Hotspots: both modules scored (no git here → churn 0, score 0);
    // the display lists the top-ranked, the CHECK decides flagging.
    assert_eq!(r.hotspots.total_scored, 2);
    assert!(r.hotspots.hotspots.iter().all(|h| h.score == 0));
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[test]
fn missing_scores_dir_is_empty_not_error() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "harna-snaps-empty-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let store = Store::open(&tmp)?;
    assert!(list_snapshots(&store)?.is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[test]
fn retention_archives_the_oldest_and_keeps_the_recent_window() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "harna-snaps-retention-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    // 14 recordings, one minute apart; keep 10 -> the 4 oldest file off.
    let mut s = assemble(100, 0, 0, Some(10), 0, 0);
    for i in 0..14 {
        s.computed_at = format!("2026-01-01T00:{i:02}:00+00:00");
        record(&store, &s)?;
    }
    let archived = archive_old_snapshots(&store, 10)?;
    assert_eq!(archived.len(), 4);
    assert_eq!(archived[0], "2026-01-01t00-00-00-00-00", "oldest first");
    let live = list_snapshots(&store)?;
    assert_eq!(live.len(), 10);
    assert_eq!(
        live[0].name, "2026-01-01t00-04-00-00-00",
        "the newest window stays live"
    );
    // Within retention: nothing moves.
    assert!(archive_old_snapshots(&store, 10)?.is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[test]
fn end_to_end_over_a_temp_store_scores_b() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "harna-score-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp)?;
    store.init()?;
    // Two components, one tested → 50% coverage → 15/30.
    let comp = |name: &str, tests: Vec<String>| Component {
        planned: false,
        name: name.into(),
        kind: ComponentKind::Primitive,
        path: format!("src/{name}.rs"),
        summary: String::new(),
        tests,
        provides: vec![],
        children: vec![],
    };
    let manifest = crate::schema::model::manifest::Manifest {
        schema_version: 1,
        layers: vec![Layer {
            isolated: false,
            name: "l".into(),
            description: String::new(),
            sequence: vec![],
            depends_on: vec![],
            role: None,
            containers: vec![Container {
                escapes: Vec::new(),
                owns: Vec::new(),
                name: "c".into(),
                path: "src".into(),
                description: String::new(),
                sequence: vec![],
                provides: vec![],
                role: None,
                gateway: None,
                connects: vec![],
                components: vec![comp("a", vec!["src/a.rs".into()]), comp("b", vec![])],
            }],
        }],
    };
    store.save_manifest(&manifest)?;
    // Fresh digest, two modules, no deps: pressure 0, no outliers.
    let mk = |path: &str| crate::search::digest::Module {
        reexports: Vec::new(),
        crate_dir: None,
        src_root: None,
        path: path.into(),
        lang: "rust".into(),
        signatures: vec![],
        deps: vec![],
        body_hash: "x".into(),
        inventory: vec![],
        mods: vec![],
    };
    store.save_digest(
        &manifest,
        &crate::search::digest::Digest {
            orphans: Vec::new(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![mk("src/a.rs"), mk("src/b.rs")],
        },
    )?;
    let s = build(&store)?;
    // 15 (coverage) + 20 (no debts) + 15 (fresh) + 15 (no pressure) + 20 (no outliers) = 85 → B.
    assert_eq!((s.score, s.grade.as_str()), (85, "B"));
    assert!(s.render().contains("score: 85 (B)"));
    assert_eq!(s.parity_open, 0);
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// deno (0.3.44): 97 (A) with 1052 modules outside every container. A
/// letter needs harna to see at least VISIBILITY_FLOOR of the map.
#[test]
fn a_repo_harna_barely_sees_gets_no_letter() {
    let mut blind = assemble(100, 0, 0, Some(10), 0, 0);
    apply_visibility(&mut blind, 0, 1052);
    assert_eq!(blind.grade, "ungraded");
    assert_eq!(blind.visible_pct, Some(0));
    let line = blind.render();
    assert!(
        line.starts_with("score: ungraded — harna sees 0%"),
        "{line}"
    );
    assert!(line.contains("harna adopt"), "{line}");

    let mut partial = assemble(100, 0, 0, Some(10), 0, 0);
    apply_visibility(&mut partial, 80, 100);
    assert_eq!(partial.grade, "A");
    assert!(partial.render().starts_with("score: 100 (A, sees 80%)"));

    let mut full = assemble(100, 0, 0, Some(10), 0, 0);
    apply_visibility(&mut full, 10, 10);
    assert!(full.render().starts_with("score: 100 (A) —"));
    // An empty map is not "unseen": nothing to see, nothing withheld.
    let mut empty = assemble(100, 0, 0, Some(10), 0, 0);
    apply_visibility(&mut empty, 0, 0);
    assert_eq!((empty.grade.as_str(), empty.visible_pct), ("A", None));
}
