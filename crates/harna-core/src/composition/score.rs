//! Component `score`: the composite health grade (0.3.12).
//!
//! Five signals, one number. Coverage (are components' tests registered?),
//! parity debts (is structural change followed through?), digest freshness
//! (is the map current?), boundary pressure (does coupling respect the
//! containers?), coupling outliers (does any module carry the map on its
//! back?). Weights are named constants with their rationale — policy as
//! data-adjacent code, argued in the open, not tuned silently.
//!
//! The grade is a snapshot-friendly struct (0.3.13 persists these); the
//! computation is pure over store-loaded inputs.

use serde::{Deserialize, Serialize};

use crate::schema::store::Store;

/// Weight rationale: coverage and coupling carry the most (30 + 20) —
/// they measure what the repo *is*; debts measure follow-through (20);
/// freshness and boundaries measure map hygiene (15 + 15).
pub const WEIGHT_COVERAGE: u8 = 30;
pub const WEIGHT_DEBTS: u8 = 20;
pub const WEIGHT_FRESHNESS: u8 = 15;
pub const WEIGHT_BOUNDARIES: u8 = 15;
pub const WEIGHT_COUPLING: u8 = 20;

/// Each open parity debt costs this much of the debts part (5+ open
/// debts zero it — follow-through has visibly lapsed by then).
pub const DEBT_COST: u8 = 4;
/// Each coupling outlier costs this much of the coupling part.
pub const OUTLIER_COST: u8 = 3;

/// The five sub-scores, each already weighted (0..=its weight).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreParts {
    pub coverage: u8,
    pub debts: u8,
    pub freshness: u8,
    pub boundaries: u8,
    pub coupling: u8,
}

/// One computed grade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub computed_at: String,
    pub coverage_pct: u8,
    pub parity_open: usize,
    /// Deferral register entries past their review date (0.3.23);
    /// defaulted for snapshots recorded before the register existed.
    #[serde(default)]
    pub deferred_overdue: usize,
    pub digest_age_secs: Option<i64>,
    pub boundary_pressure_pct: u8,
    pub outlier_count: usize,
    pub parts: ScoreParts,
    /// 0–100 composite.
    pub score: u8,
    /// A ≥ 90, B ≥ 80, C ≥ 70, D ≥ 60, else F.
    pub grade: String,
}

/// Metrics + score + hotspots together: the machine shape behind
/// `harna metrics --json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FullReport {
    pub metrics: crate::search::metrics::MetricsReport,
    pub score: Score,
    pub hotspots: crate::search::hotspots::HotspotReport,
}

impl Score {
    /// Token-efficient rendering for humans and agents.
    pub fn render(&self) -> String {
        let age = match self.digest_age_secs {
            Some(s) if s < 60 => "fresh".into(),
            Some(s) => format!("{}m old", s / 60),
            None => "missing".into(),
        };
        let debts_seg = if self.deferred_overdue > 0 {
            format!(
                "debts {}/{} ({} overdue deferred review(s))",
                self.parts.debts, WEIGHT_DEBTS, self.deferred_overdue
            )
        } else {
            format!("debts {}/{}", self.parts.debts, WEIGHT_DEBTS)
        };
        format!(
            "score: {} ({}) — coverage {}/{} · {} · freshness {}/{} ({}) · boundaries {}/{} (pressure {}%) · coupling {}/{} ({} outlier(s))\n",
            self.score,
            self.grade,
            self.parts.coverage,
            WEIGHT_COVERAGE,
            debts_seg,
            self.parts.freshness,
            WEIGHT_FRESHNESS,
            age,
            self.parts.boundaries,
            WEIGHT_BOUNDARIES,
            self.boundary_pressure_pct,
            self.parts.coupling,
            WEIGHT_COUPLING,
            self.outlier_count,
        )
    }
}

/// Grade the repo behind this store.
pub fn build(store: &Store) -> anyhow::Result<Score> {
    let manifest = store.load_manifest().unwrap_or_default();
    let digest = store.load_digest().ok();
    let metrics = digest
        .as_ref()
        .map(|d| crate::search::metrics::build(&store.repo_root(), &manifest, d));
    build_with(store, &manifest, digest.as_ref(), metrics.as_ref())
}

/// The shared assembly core (0.3.17 one-scan): coverage, parity debts,
/// freshness and the metrics-derived signals from inputs a caller may
/// already hold — `full()` passes the one map read and its metrics
/// instead of re-reading the map per signal.
pub fn build_with(
    store: &Store,
    manifest: &crate::schema::manifest::Manifest,
    digest: Option<&crate::search::digest::Digest>,
    metrics: Option<&crate::search::metrics::MetricsReport>,
) -> anyhow::Result<Score> {
    let coverage = crate::enforcement::validate::test_coverage::coverage(manifest).percent;
    let parity_open: usize = crate::enforcement::parity::list_events(store)
        .unwrap_or_default()
        .iter()
        .map(|e| e.open_targets().len())
        .sum();
    let deferred_overdue: usize = store
        .load_deferred()
        .unwrap_or_default()
        .overdue(
            env!("CARGO_PKG_VERSION"),
            &chrono::Utc::now().format("%Y-%m-%d").to_string(),
        )
        .len();
    let digest_age_secs = digest.and_then(|d| {
        chrono::DateTime::parse_from_rfc3339(&d.generated_at)
            .ok()
            .map(|t| chrono::Utc::now().signed_duration_since(t).num_seconds().max(0))
    });
    let (pressure, outliers) = metrics
        .map(|m| (m.boundary_pressure_pct, m.outliers.len()))
        .unwrap_or((0, 0));
    Ok(assemble(
        coverage,
        parity_open,
        deferred_overdue,
        digest_age_secs,
        pressure,
        outliers,
    ))
}

/// Metrics + score + hotspots in one call (the `harna metrics` machine shape).
///
/// One scan, many consumers (0.3.17): metrics, score and hotspots all
/// measure the same committed map — it is read once and passed down, not
/// parsed per signal (the whole-map era stacked four reads here).
pub fn full(store: &Store) -> anyhow::Result<FullReport> {
    let manifest = store.load_manifest().unwrap_or_default();
    let digest = store.load_digest()?;
    let metrics = crate::search::metrics::build(&store.repo_root(), &manifest, &digest);
    let score = build_with(store, &manifest, Some(&digest), Some(&metrics))?;
    let hotspots = crate::search::hotspots::build(&store.repo_root(), &digest);
    Ok(FullReport { metrics, score, hotspots })
}

/// A drop of this many points between consecutive recordings is a
/// regression (named, not silent). Smaller drifts still print their
/// delta — only the flag has a threshold.
pub const REGRESSION_DROP: i16 = 5;

/// Retention (0.3.18 — the 0.3.14 deferral): the newest 12 recordings
/// stay live, older ones archive. A year of bursts is the trend window
/// the grade reasons about; beyond it the drift it measures is archaeology.
pub const SNAPSHOT_RETENTION: usize = 12;

/// Archive the snapshots past the retention window (0.3.18): names sort
/// chronologically, so the newest `keep` stay live and the rest move to
/// `.harna/archive/scores/`. Returns the archived names.
pub fn archive_old_snapshots(store: &Store, keep: usize) -> anyhow::Result<Vec<String>> {
    let names = list_snapshots(store)?; // oldest first (sorted stems)
    let mut archived = Vec::new();
    if names.len() > keep {
        for snap in &names[..names.len() - keep] {
            let path = store.scores_dir().join(format!("{}.toml", snap.name));
            store.archive_file(&path, "scores")?;
            archived.push(snap.name.clone());
        }
    }
    Ok(archived)
}

/// One persisted recording: filename stem + the grade it holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub name: String,
    pub score: Score,
}

/// One trend point: the recording plus its move since the previous one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrendRow {
    pub name: String,
    pub score: u8,
    pub grade: String,
    /// Move since the previous recording; None for the first.
    pub delta: Option<i16>,
    pub regression: bool,
    /// Which parts moved, as `"coverage -11"` entries (0.3.14 — the
    /// attribution 0.3.13 deferred: snapshots always stored parts, so
    /// this works retroactively over every recording).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moved: Vec<String>,
}

/// Timestamp slug for a recording filename: the score's own
/// `computed_at`, filesystem-safe (same sanitizer shape as decision
/// slugs — lowercase alphanumerics survive, the rest becomes `-`).
pub fn snapshot_slug(computed_at: &str) -> String {
    computed_at
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Persist this grade as a timestamped snapshot. The file IO lives in
/// the store (the `fs:write` gateway); this names the stem and serializes.
/// A same-stamp collision takes the next `_N` suffix instead of
/// overwriting — recordings append, never replace.
pub fn record(store: &Store, score: &Score) -> anyhow::Result<std::path::PathBuf> {
    let stem = snapshot_slug(&score.computed_at);
    let stem = if stem.is_empty() { "score".into() } else { stem };
    let raw = toml::to_string_pretty(score)?;
    store.save_score_snapshot(&stem, &raw)
}

/// All recordings, oldest first (timestamp slugs sort chronologically).
/// A missing dir is no recordings, not an error; a corrupt file fails
/// loudly — silent rot would poison every trend drawn from it.
pub fn list_snapshots(store: &Store) -> anyhow::Result<Vec<Snapshot>> {
    let mut out = Vec::new();
    for name in store.score_snapshot_names() {
        let raw = store.read_score_snapshot(&name)?;
        let score: Score = toml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("parsing {name}.toml: {e}"))?;
        out.push(Snapshot { name, score });
    }
    Ok(out)
}

/// Pure trend assembly: deltas between consecutive recordings, the
/// regression flag at the threshold, and part-level attribution of each
/// move (which sub-score carried the drift).
pub fn trend(snapshots: &[Snapshot]) -> Vec<TrendRow> {
    let mut rows = Vec::new();
    let mut prev: Option<&Score> = None;
    for s in snapshots {
        let delta = prev.map(|p| s.score.score as i16 - p.score as i16);
        let moved = prev.map(|p| parts_moved(&p.parts, &s.score.parts)).unwrap_or_default();
        rows.push(TrendRow {
            name: s.name.clone(),
            score: s.score.score,
            grade: s.score.grade.clone(),
            regression: delta.map_or(false, |d| d <= -REGRESSION_DROP),
            moved,
            delta,
        });
        prev = Some(&s.score);
    }
    rows
}

/// Sub-score moves between two recordings, fixed part order, nonzero
/// only — the attribution behind a delta.
fn parts_moved(before: &ScoreParts, after: &ScoreParts) -> Vec<String> {
    [
        ("coverage", before.coverage, after.coverage),
        ("debts", before.debts, after.debts),
        ("freshness", before.freshness, after.freshness),
        ("boundaries", before.boundaries, after.boundaries),
        ("coupling", before.coupling, after.coupling),
    ]
    .into_iter()
    .filter_map(|(name, b, a)| {
        let d = a as i16 - b as i16;
        (d != 0).then(|| format!("{name} {d:+}"))
    })
    .collect()
}

impl TrendRow {
    fn render(&self) -> String {
        let move_note = match self.delta {
            None => "first recording".into(),
            Some(d) if d == 0 => "= last".into(),
            Some(d) => format!("{d:+} vs last"),
        };
        let flag = if self.regression { " ▼ REGRESSION" } else { "" };
        let moved = if self.moved.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.moved.join(", "))
        };
        format!("  {}: {} ({}) [{move_note}]{flag}{moved}\n", self.name, self.score, self.grade)
    }
}

/// Token-efficient trend view for humans and agents.
pub fn render_trend(rows: &[TrendRow]) -> String {
    if rows.is_empty() {
        return "trends: no snapshots yet — `harna metrics --record` stores the first\n".into();
    }
    let mut out = format!("trends: {} recording(s)\n", rows.len());
    for r in rows {
        out.push_str(&r.render());
    }
    out
}

/// Pure assembly: inputs in, grade out (every weight decision testable
/// without a filesystem). Overdue deferred-register reviews cost a debt
/// each (0.3.23) — a missed review date is lapsed follow-through, the
/// same weight class as an unresolved parity target.
pub fn assemble(
    coverage_pct: u8,
    parity_open: usize,
    deferred_overdue: usize,
    digest_age_secs: Option<i64>,
    boundary_pressure_pct: u8,
    outlier_count: usize,
) -> Score {
    let coverage = (coverage_pct as u16 * WEIGHT_COVERAGE as u16 / 100).min(WEIGHT_COVERAGE as u16) as u8;
    let debt_sources = (parity_open as u8)
        .saturating_add(deferred_overdue as u8);
    let debts = WEIGHT_DEBTS.saturating_sub(debt_sources.saturating_mul(DEBT_COST).min(WEIGHT_DEBTS));
    let freshness = freshness_points(digest_age_secs);
    let boundaries =
        (WEIGHT_BOUNDARIES as u16 * (100 - boundary_pressure_pct.min(100)) as u16 / 100) as u8;
    let coupling =
        WEIGHT_COUPLING.saturating_sub((outlier_count as u8).saturating_mul(OUTLIER_COST).min(WEIGHT_COUPLING));
    let score: u8 =
        (coverage as u16 + debts as u16 + freshness as u16 + boundaries as u16 + coupling as u16)
            .min(100) as u8;
    Score {
        computed_at: chrono::Utc::now().to_rfc3339(),
        coverage_pct,
        parity_open,
        deferred_overdue,
        digest_age_secs,
        boundary_pressure_pct,
        outlier_count,
        parts: ScoreParts { coverage, debts, freshness, boundaries, coupling },
        grade: grade_for(score).into(),
        score,
    }
}

/// Freshness buckets: a map younger than 5 minutes is current; past a
/// week it no longer informs (0). Missing digest scores 0 — the map the
/// grade leans on does not exist.
pub fn freshness_points(age_secs: Option<i64>) -> u8 {
    match age_secs {
        Some(s) if s < 300 => WEIGHT_FRESHNESS,
        Some(s) if s < 3_600 => 12,
        Some(s) if s < 86_400 => 8,
        Some(s) if s < 604_800 => 4,
        _ => 0,
    }
}

pub fn grade_for(score: u8) -> &'static str {
    if score >= 90 {
        "A"
    } else if score >= 80 {
        "B"
    } else if score >= 70 {
        "C"
    } else if score >= 60 {
        "D"
    } else {
        "F"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::manifest::{Component, ComponentKind, Container, Layer};

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
        assert_eq!(freshness_points(Some(300)), 12, "boundary is exclusive below");
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
        assert_eq!(s.parts, ScoreParts { coverage: 30, debts: 20, freshness: 15, boundaries: 15, coupling: 20 });
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
        assert_eq!(snapshot_slug("2026-10-16T12:34:56.789+00:00"), "2026-10-16t12-34-56-789-00-00");
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
        assert!(p2.ends_with("2026-10-16t12-00-00-00-00_1.toml"), "append, never replace");
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
            Snapshot { name: name.into(), score: s }
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
            Snapshot { name: name.into(), score: s }
        };
        // a: 30+20+15+15+20 = 100; b: 15+12+15+15+20 = 77.
        let snaps = vec![snap("a", 100, 0), snap("b", 50, 2)];
        let rows = trend(&snaps);
        assert!(rows[0].moved.is_empty(), "first recording attributes nothing");
        assert_eq!(rows[1].delta, Some(-23));
        assert_eq!(rows[1].moved, vec!["coverage -15".to_string(), "debts -8".to_string()]);
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
        let manifest = crate::schema::manifest::Manifest {
            schema_version: 1,
            layers: vec![crate::schema::manifest::Layer {
                name: "l".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                  role: None,
                containers: vec![crate::schema::manifest::Container {
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
                generated_at: chrono::Utc::now().to_rfc3339(),
                extractor_version: crate::search::digest::EXTRACTOR_VERSION,
                modules: vec![mk("src/a.rs"), mk("src/b.rs")],
            },
        )?;

        let r = full(&store)?;
        // Metrics: two modules, no edges, no pressure.
        assert_eq!(r.metrics.modules.len(), 2);
        assert_eq!((r.metrics.total_edges, r.metrics.boundary_pressure_pct), (0, 0));
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
        assert_eq!(live[0].name, "2026-01-01t00-04-00-00-00", "the newest window stays live");
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
            name: name.into(),
            kind: ComponentKind::Primitive,
            path: format!("src/{name}.rs"),
            summary: String::new(),
            tests,
            provides: vec![],
            children: vec![],
        };
        let manifest = crate::schema::manifest::Manifest {
            schema_version: 1,
            layers: vec![Layer {
                name: "l".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                  role: None,
                containers: vec![Container {
                    name: "c".into(),
                    path: "src".into(),
                    description: String::new(),
                    sequence: vec![],
                    provides: vec![],
                    role: None,
                    gateway: None,
                    connects: vec![],
                    components: vec![
                        comp("a", vec!["src/a.rs".into()]),
                        comp("b", vec![]),
                    ],
                }],
            }],
        };
        store.save_manifest(&manifest)?;
        // Fresh digest, two modules, no deps: pressure 0, no outliers.
        let mk = |path: &str| crate::search::digest::Module {
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
}
