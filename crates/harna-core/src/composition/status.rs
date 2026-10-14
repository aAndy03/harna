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
        format!(
            "harna {version} · schema v{schema} ({note})\n\
             map: {modules} modules, {signatures} signatures · digest {digest}\n\
             manifest: {layers} layers / {containers} containers / {components} components\n\
             rules: {rules} ({env} env-scoped) · parity: {events} events, {open} open debts\n{handoff}",
            version = self.version,
            schema = self.schema_version,
            note = schema_note,
            digest = digest,
            modules = self.modules,
            signatures = self.signatures,
            layers = self.layers,
            containers = self.containers,
            components = self.components,
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
        let tmp = std::env::temp_dir().join(format!("harna-status-{}", std::process::id()));
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
