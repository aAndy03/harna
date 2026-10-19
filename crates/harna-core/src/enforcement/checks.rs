//! Component `check-registry`: the single loader for the check registry data
//! source (checks.toml). Every consumer of check semantics — rule binding
//! validation, severity overrides, the doctor's binding coherence, the
//! CLI/MCP add-rule edges — imports this loader and nothing else; the toml
//! file is the point of truth and stays code-free.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::enforcement::violation::Severity;

pub const VALID_SEVERITIES: &str = "error | info";

#[derive(Debug, Clone, Deserialize)]
struct RawRegistry {
    #[serde(default, rename = "checks")]
    checks: Vec<RawCheck>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCheck {
    id: String,
    #[serde(default)]
    description: String,
    #[serde(default = "raw_default_severity")]
    severity: String,
    /// The check reads a bound rule's `limit` (a budget).
    #[serde(default)]
    takes_limit: bool,
    /// The check gates only where a live rule binds it (0.3.44).
    #[serde(default)]
    adopted_by_binding: bool,
    /// The check reads a bound rule's `values` list (0.3.45).
    #[serde(default)]
    takes_values: bool,
    /// The check reads a bound rule's `pattern` as its own argument, not
    /// as a naming convention (0.3.45: sync-group, file-header).
    #[serde(default)]
    takes_pattern: bool,
}

fn raw_default_severity() -> String {
    "error".into()
}

/// One check's semantics as data: id, what it enforces, default severity.
#[derive(Debug, Clone)]
pub struct CheckSpec {
    pub id: String,
    pub description: String,
    pub default_severity: Severity,
    /// Whether a rule bound to this check may carry a `limit`.
    pub takes_limit: bool,
    /// Content checks that need no manifest declaration would otherwise
    /// gate a repo on day one (0.3.44, deno: 174 URL errors nobody asked
    /// for). These advise until a live rule binds them (or is named after
    /// them); then the registry severity — or the rule's override — holds.
    pub adopted_by_binding: bool,
    pub takes_values: bool,
    pub takes_pattern: bool,
}

impl CheckSpec {
    pub fn severity_word(&self) -> &'static str {
        match self.default_severity {
            Severity::Error => "error",
            Severity::Info => "info",
        }
    }
}

/// The whole registry, parsed once (declaration order from the data file).
pub fn all() -> &'static [CheckSpec] {
    static REGISTRY: OnceLock<Vec<CheckSpec>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let raw: RawRegistry = toml::from_str(include_str!("checks.toml"))
            .expect("checks.toml must be valid data (no code)");
        raw.checks
            .into_iter()
            .map(|c| CheckSpec {
                description: c.description,
                default_severity: parse_severity(&c.severity)
                    .expect("checks.toml severity values must come from the accepted set"),
                takes_limit: c.takes_limit,
                adopted_by_binding: c.adopted_by_binding,
                takes_values: c.takes_values,
                takes_pattern: c.takes_pattern,
                id: c.id,
            })
            .collect()
    })
}

pub fn spec(id: &str) -> Option<&'static CheckSpec> {
    all().iter().find(|c| c.id == id)
}

pub fn known(id: &str) -> bool {
    spec(id).is_some()
}

/// Does a rule bound to `id` accept a `limit`? One answer for both edges
/// (CLI and MCP add-rule) — the list is registry data, not code.
/// A check's registry severity (Error when the id is unknown — an
/// unregistered check must not pass silently). One lookup for every check
/// (0.3.45: eleven copies of the same three lines).
pub fn severity_of(id: &str) -> crate::enforcement::violation::Severity {
    spec(id)
        .map(|s| s.default_severity)
        .unwrap_or(crate::enforcement::violation::Severity::Error)
}

/// Whether `id` reads a rule's `pattern` as its own argument.
pub fn takes_pattern(id: &str) -> bool {
    spec(id).is_some_and(|s| s.takes_pattern)
}

pub fn takes_limit(id: &str) -> bool {
    spec(id).is_some_and(|s| s.takes_limit)
}

/// The budget checks, comma-joined — for the add-rule hint.
pub fn limit_ids_listed() -> String {
    all()
        .iter()
        .filter(|c| c.takes_limit)
        .map(|c| c.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The known ids, comma-joined — for hints at the edges (CLI, MCP, validate).
pub fn known_ids_listed() -> String {
    all()
        .iter()
        .map(|c| c.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Parse a severity word as written in rules.toml or the CLI/MCP edge.
pub fn parse_severity(word: &str) -> Option<Severity> {
    match word.trim().to_ascii_lowercase().as_str() {
        "error" => Some(Severity::Error),
        "info" => Some(Severity::Info),
        _ => None,
    }
}

/// Validate a pattern rule's regex (the CLI/MCP edges call this before
/// saving; hand-edited files surface it as a validate violation).
pub fn compile_pattern(pattern: &str) -> Result<(), String> {
    regex::Regex::new(pattern)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_the_canonical_source() {
        let all = all();
        assert_eq!(all.len(), 39, "the shipped rule-bindable checks");
        assert!(all.iter().all(|c| !c.description.is_empty()));
        // Every id is unique.
        let mut ids: Vec<_> = all.iter().map(|c| c.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
    }

    #[test]
    fn default_severities_match_the_emission_sites() {
        // Defaults must agree with what the identical check emits today,
        // so "registry as data" changes no behavior for existing repos.
        assert_eq!(
            spec("no-hardcoded-urls").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(
            spec("layer-boundary").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(
            spec("manifest-path").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(
            spec("manifest-sequence").unwrap().default_severity,
            Severity::Info
        );
        assert_eq!(
            spec("manifest-depends").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(
            spec("parity-open").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(
            spec("component-granularity").unwrap().default_severity,
            Severity::Info
        );
        assert_eq!(
            spec("tests-declared").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(
            spec("no-test-update").unwrap().default_severity,
            Severity::Info
        );
        assert_eq!(
            spec("decision-chain").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(spec("pattern").unwrap().default_severity, Severity::Info);
        assert_eq!(
            spec("declared-surface").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(spec("no-cycles").unwrap().default_severity, Severity::Error);
        assert_eq!(
            spec("engine-own-container").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(
            spec("container-size").unwrap().default_severity,
            Severity::Info
        );
        assert_eq!(
            spec("fan-out-cap").unwrap().default_severity,
            Severity::Info
        );
        assert_eq!(
            spec("no-copy-paste").unwrap().default_severity,
            Severity::Info
        );
        assert_eq!(
            spec("single-owner").unwrap().default_severity,
            Severity::Info
        );
        assert_eq!(spec("no-bypass").unwrap().default_severity, Severity::Error);
        assert_eq!(
            spec("depth-budget").unwrap().default_severity,
            Severity::Info
        );
        assert_eq!(
            spec("gateway-ownership").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(
            spec("no-sideways").unwrap().default_severity,
            Severity::Error
        );
        assert_eq!(
            spec("transiter-thinness").unwrap().default_severity,
            Severity::Info
        );
        assert_eq!(spec("hotspot").unwrap().default_severity, Severity::Info);
        assert_eq!(
            spec("rules-coverage").unwrap().default_severity,
            Severity::Info
        );
        assert_eq!(spec("born-size").unwrap().default_severity, Severity::Error);
        assert_eq!(
            spec("component-granularity").unwrap().severity_word(),
            "info"
        );
    }

    #[test]
    fn severity_words_parse_snugly() {
        assert_eq!(parse_severity("error"), Some(Severity::Error));
        assert_eq!(parse_severity("INFO"), Some(Severity::Info));
        assert_eq!(parse_severity(" info "), Some(Severity::Info));
        assert_eq!(parse_severity("warn"), None);
        assert_eq!(parse_severity(""), None);
        assert!(known("decision-chain"));
        assert!(!known("decision-chain-note"));
    }
}
