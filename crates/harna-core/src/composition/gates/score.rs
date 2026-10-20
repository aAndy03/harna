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

/// The five sub-scores, each already weighted (0..=its weight). Every
/// part defaults (0.9.0): a part added later must not break old snapshots.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
    /// A ≥ 90, B ≥ 80, C ≥ 70, D ≥ 60, else F — or "ungraded" when harna
    /// sees too little of the repo to stand behind a letter (0.3.44).
    pub grade: String,
    /// Share of map modules inside some container (0.3.44). None: no map
    /// measured, or a snapshot recorded before visibility existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_pct: Option<u8>,
}

/// Below this share of modules inside a container, the grade is withheld
/// (0.3.44, deno: 97 (A) on a repo whose 1052 modules sat outside every
/// container — every sub-score was vacuously full).
pub const VISIBILITY_FLOOR: u8 = 50;

/// Record how much of the map the manifest reaches; withhold the letter
/// below the floor. The number stays (trends keep their series).
pub fn apply_visibility(score: &mut Score, homed: usize, total: usize) {
    if total == 0 {
        return;
    }
    let pct = (homed * 100 / total).min(100) as u8;
    score.visible_pct = Some(pct);
    if pct < VISIBILITY_FLOOR {
        score.grade = "ungraded".into();
    }
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
        let head = match self.visible_pct {
            Some(v) if self.grade == "ungraded" => format!(
                "score: ungraded — harna sees {v}% of this repo's modules (inside a container); \
                 below {VISIBILITY_FLOOR}% every part reads vacuously full. \
                 `harna adopt` proposes the containers. Parts as measured: {}",
                self.score
            ),
            Some(v) if v < 100 => format!("score: {} ({}, sees {v}%)", self.score, self.grade),
            _ => format!("score: {} ({})", self.score, self.grade),
        };
        format!(
            "{head} — coverage {}/{} · {} · freshness {}/{} ({}) · boundaries {}/{} (pressure {}%) · coupling {}/{} ({} outlier(s))\n",
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
    manifest: &crate::schema::model::manifest::Manifest,
    digest: Option<&crate::search::digest::Digest>,
    metrics: Option<&crate::search::metrics::MetricsReport>,
) -> anyhow::Result<Score> {
    let coverage = {
        use crate::enforcement::validate::coherence::test_coverage as tc;
        tc::coverage_with(
            manifest,
            &tc::by_kind::covered(&store.repo_root(), manifest),
        )
        .percent
    };
    let parity_open: usize = crate::enforcement::parity::list_events(store)
        .unwrap_or_default()
        .iter()
        .map(|e| e.open_targets().len())
        .sum();
    let roadmap = store.load_roadmap().unwrap_or_default();
    let deferred_overdue: usize = store
        .load_deferred()
        .unwrap_or_default()
        .overdue(
            &crate::enforcement::validate::artifacts::deprecation::project_version(
                &store.repo_root(),
            )
            .unwrap_or_default(),
            &chrono::Utc::now().format("%Y-%m-%d").to_string(),
            &|id| roadmap.reached(id),
            &|id| roadmap.find(id).is_some(),
        )
        .len();
    let digest_age_secs = digest.and_then(|d| {
        chrono::DateTime::parse_from_rfc3339(&d.generated_at)
            .ok()
            .map(|t| {
                chrono::Utc::now()
                    .signed_duration_since(t)
                    .num_seconds()
                    .max(0)
            })
    });
    let (pressure, outliers) = metrics
        .map(|m| (m.boundary_pressure_pct, m.outliers.len()))
        .unwrap_or((0, 0));
    let mut score = assemble(
        coverage,
        parity_open,
        deferred_overdue,
        digest_age_secs,
        pressure,
        outliers,
    );
    if let Some(m) = metrics {
        apply_visibility(
            &mut score,
            m.modules.len().saturating_sub(m.unhomed),
            m.modules.len(),
        );
    }
    Ok(score)
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
    Ok(FullReport {
        metrics,
        score,
        hotspots,
    })
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
    let stem = if stem.is_empty() {
        "score".into()
    } else {
        stem
    };
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
        let score: Score =
            toml::from_str(&raw).map_err(|e| anyhow::anyhow!("parsing {name}.toml: {e}"))?;
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
        let moved = prev
            .map(|p| parts_moved(&p.parts, &s.score.parts))
            .unwrap_or_default();
        rows.push(TrendRow {
            name: s.name.clone(),
            score: s.score.score,
            grade: s.score.grade.clone(),
            regression: delta.is_some_and(|d| d <= -REGRESSION_DROP),
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
            Some(0) => "= last".into(),
            Some(d) => format!("{d:+} vs last"),
        };
        let flag = if self.regression {
            " ▼ REGRESSION"
        } else {
            ""
        };
        let moved = if self.moved.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.moved.join(", "))
        };
        format!(
            "  {}: {} ({}) [{move_note}]{flag}{moved}\n",
            self.name, self.score, self.grade
        )
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
    let coverage =
        (coverage_pct as u16 * WEIGHT_COVERAGE as u16 / 100).min(WEIGHT_COVERAGE as u16) as u8;
    let debt_sources = (parity_open as u8).saturating_add(deferred_overdue as u8);
    let debts =
        WEIGHT_DEBTS.saturating_sub(debt_sources.saturating_mul(DEBT_COST).min(WEIGHT_DEBTS));
    let freshness = freshness_points(digest_age_secs);
    let boundaries =
        (WEIGHT_BOUNDARIES as u16 * (100 - boundary_pressure_pct.min(100)) as u16 / 100) as u8;
    let coupling = WEIGHT_COUPLING.saturating_sub(
        (outlier_count as u8)
            .saturating_mul(OUTLIER_COST)
            .min(WEIGHT_COUPLING),
    );
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
        parts: ScoreParts {
            coverage,
            debts,
            freshness,
            boundaries,
            coupling,
        },
        grade: grade_for(score).into(),
        score,
        visible_pct: None,
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
mod tests;
