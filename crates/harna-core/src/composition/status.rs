//! Component `status`: the one-shot health line.
//!
//! One artifact every future 0.2.x machine output builds on: agents and
//! shell scripts get the repo's harna state in one call — map size, rules,
//! parity debts, digest freshness — without composing five commands.

use serde::Serialize;

use crate::schema::manifest::Manifest;
use crate::schema::store::Store;

#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    /// harna tool version (this workspace's package version).
    pub version: String,
    /// Schema stamp of the repo's manifest.
    pub schema_version: u32,
    /// True unless the repo was written by a newer harna.
    pub schema_current: bool,
    pub modules: usize,
    pub signatures: usize,
    pub layers: usize,
    pub containers: usize,
    pub components: usize,
    /// Components registering at least one test path (0.3.11).
    pub components_with_tests: usize,
    /// Floored percent of components with registered tests (0.3.11).
    pub test_coverage_pct: u8,
    /// Names of components with no registered tests (0.3.11).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub untested_components: Vec<String>,
    /// Composite health grade 0–100 (0.3.12; breakdown in `harna metrics`).
    pub score: u8,
    /// Letter grade: A ≥ 90, B ≥ 80, C ≥ 70, D ≥ 60, else F (0.3.12).
    pub grade: String,
    pub rules: usize,
    pub env_scoped_rules: usize,
    pub parity_events: usize,
    pub parity_open_debts: usize,
    /// Digest age in seconds; None = no digest or unparseable stamp.
    pub digest_age_secs: Option<i64>,
    /// The burst journal's health: next steps waiting at session start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff_next: Option<usize>,
}

impl StatusReport {
    /// The health line: compact, agent-renderable.
    pub fn render(&self) -> String {
        let schema_note = if self.schema_current { "current" } else { "TOO NEW — update harna" };
        let digest = match self.digest_age_secs {
            Some(s) if s < 60 => format!("fresh"),
            Some(s) => format!("{}m old", s / 60),
            None => "missing — run `harna scan`".into(),
        };
        let handoff = match self.handoff_next {
            Some(0) | None => String::new(),
            Some(n) => format!(
                "handoff on disk: {n} declared next step(s) — `harna handoff` (or onboarding) shows the summary\n"
            ),
        };
        let untested = if self.untested_components.is_empty() {
            String::new()
        } else {
            const SHOW: usize = 10;
            let mut names: Vec<&str> =
                self.untested_components.iter().map(|s| s.as_str()).collect();
            names.sort_unstable();
            let shown = names.iter().take(SHOW).copied().collect::<Vec<_>>().join(", ");
            let rest = names.len().saturating_sub(SHOW);
            let more = if rest == 0 { String::new() } else { format!(" …and {rest} more") };
            format!("untested: {shown}{more}\n")
        };
        format!(
            "harna {version} · schema v{schema} ({note})\n\
             map: {modules} modules, {signatures} signatures · digest {digest}\n\
             manifest: {layers} layers / {containers} containers / {components} components\n\
             tests: {covered}/{components} components ({pct}%)\n\
             {untested}\
             score: {score} ({grade}) · rules: {rules} ({env} env-scoped) · parity: {events} events, {open} open debts\n{handoff}",
            version = self.version,
            schema = self.schema_version,
            note = schema_note,
            digest = digest,
            modules = self.modules,
            signatures = self.signatures,
            layers = self.layers,
            containers = self.containers,
            components = self.components,
            covered = self.components_with_tests,
            pct = self.test_coverage_pct,
            untested = untested,
            score = self.score,
            grade = self.grade,
            rules = self.rules,
            env = self.env_scoped_rules,
            events = self.parity_events,
            open = self.parity_open_debts,
            handoff = handoff,
        )
    }
}

pub fn build(store: &Store) -> anyhow::Result<StatusReport> {
    let manifest: Manifest = store.load_manifest().unwrap_or_default();
    let rules = store.load_rules().unwrap_or_default();
    let cov = crate::enforcement::validate::test_coverage::coverage(&manifest);
    let sc = crate::composition::score::build(store)?;

    let events = crate::enforcement::parity::list_events(store).unwrap_or_default();
    let parity_open = events.iter().map(|e| e.open_targets().len()).sum();

    let (modules, signatures, digest_age) = match store.load_digest() {
        Ok(d) => (
            d.module_count(),
            d.signature_count(),
            chrono::DateTime::parse_from_rfc3339(&d.generated_at)
                .ok()
                .map(|t| chrono::Utc::now().signed_duration_since(t).num_seconds().max(0)),
        ),
        Err(_) => (0, 0, None),
    };

    Ok(StatusReport {
        version: env!("CARGO_PKG_VERSION").into(),
        schema_version: manifest.schema_version,
        schema_current: manifest.schema_version <= crate::SCHEMA_VERSION,
        modules,
        signatures,
        layers: manifest.layers.len(),
        containers: manifest.container_count(),
        components: manifest.component_count(),
        components_with_tests: cov.with_tests,
        test_coverage_pct: cov.percent,
        untested_components: cov.untested,
        score: sc.score,
        grade: sc.grade,
        rules: rules.rules.len(),
        env_scoped_rules: rules.rules.iter().filter(|r| r.env.is_some()).count(),
        parity_events: events.len(),
        parity_open_debts: parity_open,
        digest_age_secs: digest_age,
        handoff_next: crate::schema::handoff::load(store).map(|h| h.declared_next.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_counts_and_renders() {
        let tmp = std::env::temp_dir().join(format!(
            "harna-status-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        // Fresh dir every run: pid reuse across runs would otherwise resurrect
        // a previous run's handoff and fail the no-burst assertion below.
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        // A rule with env and one without.
        let toml_src = r#"
schema_version = 1

[[rules]]
id = "global-rule"
text = "t"
rationale = "r"
origin = "test"

[[rules]]
id = "scoped-rule"
text = "t"
rationale = "r"
origin = "test"
env = "dev"
"#;
        std::fs::write(store.rules_path(), toml_src).unwrap();

        let s = build(&store).unwrap();
        assert_eq!(s.rules, 2);
        assert_eq!(s.env_scoped_rules, 1);
        assert_eq!(s.modules, 0, "no digest yet");
        assert_eq!(s.digest_age_secs, None);
        assert!(s.schema_current);
        assert_eq!(s.handoff_next, None, "no burst recorded yet");
        // Empty manifest: coverage is vacuously full, nothing untested.
        assert_eq!((s.components, s.components_with_tests, s.test_coverage_pct), (0, 0, 100));
        assert!(s.untested_components.is_empty());
        assert!(s.render().contains("tests: 0/0 components (100%)"));
        // No digest, no debts: 30 + 20 + 0 + 15 + 20 = 85 → B.
        assert_eq!((s.score, s.grade.as_str()), (85, "B"));
        assert!(s.render().contains("score: 85 (B)"));

        // A recorded burst surfaces on the health line: the resumed session
        // sees the declared next steps before reading anything else.
        crate::schema::handoff::save(
            &store,
            "smoke burst",
            &["resume A".to_string(), "resume B".to_string()],
            1,
        )
        .unwrap();
        let s = build(&store).unwrap();
        assert_eq!(s.handoff_next, Some(2));
        let text = s.render();
        assert!(text.contains("handoff on disk: 2 declared next step(s)"));

        let text = s.render();
        assert!(text.contains("harna "));
        assert!(text.contains("2"));
        assert!(text.contains("digest missing"));
        // machine output is round-trippable
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"env_scoped_rules\":1"));
    }
}
