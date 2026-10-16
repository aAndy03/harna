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
    pub digest_age_secs: Option<i64>,
    pub boundary_pressure_pct: u8,
    pub outlier_count: usize,
    pub parts: ScoreParts,
    /// 0–100 composite.
    pub score: u8,
    /// A ≥ 90, B ≥ 80, C ≥ 70, D ≥ 60, else F.
    pub grade: String,
}

/// Metrics + score together: the machine shape behind `harna metrics --json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FullReport {
    pub metrics: crate::search::metrics::MetricsReport,
    pub score: Score,
}

impl Score {
    /// Token-efficient rendering for humans and agents.
    pub fn render(&self) -> String {
        let age = match self.digest_age_secs {
            Some(s) if s < 60 => "fresh".into(),
            Some(s) => format!("{}m old", s / 60),
            None => "missing".into(),
        };
        format!(
            "score: {} ({}) — coverage {}/{} · debts {}/{} · freshness {}/{} ({}) · boundaries {}/{} (pressure {}%) · coupling {}/{} ({} outlier(s))\n",
            self.score,
            self.grade,
            self.parts.coverage,
            WEIGHT_COVERAGE,
            self.parts.debts,
            WEIGHT_DEBTS,
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
    let coverage = crate::enforcement::validate::test_coverage::coverage(&manifest).percent;
    let parity_open: usize = crate::enforcement::parity::list_events(store)
        .unwrap_or_default()
        .iter()
        .map(|e| e.open_targets().len())
        .sum();
    let (digest_age_secs, metrics) = match store.load_digest() {
        Ok(d) => {
            let age = chrono::DateTime::parse_from_rfc3339(&d.generated_at)
                .ok()
                .map(|t| chrono::Utc::now().signed_duration_since(t).num_seconds().max(0));
            let m = crate::search::metrics::build(&store.repo_root(), &manifest, &d);
            (age, Some(m))
        }
        Err(_) => (None, None),
    };
    let (pressure, outliers) = metrics
        .as_ref()
        .map(|m| (m.boundary_pressure_pct, m.outliers.len()))
        .unwrap_or((0, 0));
    Ok(assemble(coverage, parity_open, digest_age_secs, pressure, outliers))
}

/// Metrics + score in one call (the `harna metrics` machine shape).
pub fn full(store: &Store) -> anyhow::Result<FullReport> {
    let metrics = crate::search::metrics::build_store(store)?;
    let score = build(store)?;
    Ok(FullReport { metrics, score })
}

/// Pure assembly: inputs in, grade out (every weight decision testable
/// without a filesystem).
pub fn assemble(
    coverage_pct: u8,
    parity_open: usize,
    digest_age_secs: Option<i64>,
    boundary_pressure_pct: u8,
    outlier_count: usize,
) -> Score {
    let coverage = (coverage_pct as u16 * WEIGHT_COVERAGE as u16 / 100).min(WEIGHT_COVERAGE as u16) as u8;
    let debts = WEIGHT_DEBTS.saturating_sub((parity_open as u8).saturating_mul(DEBT_COST).min(WEIGHT_DEBTS));
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
        let s = assemble(100, 0, Some(10), 0, 0);
        assert_eq!((s.score, s.grade.as_str()), (100, "A"));
        assert_eq!(s.parts, ScoreParts { coverage: 30, debts: 20, freshness: 15, boundaries: 15, coupling: 20 });
        // Debts and outliers saturate instead of underflowing.
        let s = assemble(0, 999, None, 100, 999);
        assert_eq!((s.parts.debts, s.parts.coupling), (0, 0));
        assert_eq!((s.score, s.grade.as_str()), (0, "F"));
    }

    #[test]
    fn coverage_floors_through_the_assembly() {
        // 2/3 → 66 → 66*30/100 = 19 (floored twice, never flattered).
        let s = assemble(66, 0, Some(10), 0, 0);
        assert_eq!(s.parts.coverage, 19);
        assert_eq!(s.score, 19 + 20 + 15 + 15 + 20);
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
        store.save_digest(&crate::search::digest::Digest {
            generated_at: chrono::Utc::now().to_rfc3339(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![mk("src/a.rs"), mk("src/b.rs")],
        })?;
        let s = build(&store)?;
        // 15 (coverage) + 20 (no debts) + 15 (fresh) + 15 (no pressure) + 20 (no outliers) = 85 → B.
        assert_eq!((s.score, s.grade.as_str()), (85, "B"));
        assert!(s.render().contains("score: 85 (B)"));
        assert_eq!(s.parity_open, 0);
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }
}
