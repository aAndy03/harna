//! Rules: scoped, persistent coding rules with rationale and origin.
//!
//! Every rule records *why* it exists and *who/when* created it, so a fresh
//! agent session understands the rule, not just its text — without asking.

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::schema::manifest::current_schema_version;

/// Top-level rules document (`.harna/rules.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rules {
    /// Data schema version; written by the tool, migrated by `harna migrate`.
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
}

impl Default for Rules {
    fn default() -> Self {
        Self { schema_version: crate::SCHEMA_VERSION, rules: Vec::new() }
    }
}

/// Check ids the enforceable checks expose today. A rule may bind to one —
/// documentation (why) and enforcement (what) share the id.
pub const BUILTIN_CHECKS: &[&str] = &[
    "no-hardcoded-urls",
    "layer-boundary",
    "manifest-path",
    "manifest-sequence",
    "manifest-depends",
    "parity-open",
    "component-granularity",
    "tests-declared",
    "no-test-update",
    "decision-chain",
];

/// A single coding rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Stable id, e.g. "no-hardcoded-urls".
    pub id: String,
    /// The rule in imperative form, e.g. "Use `Endpoint` constants; never hardcode URLs".
    pub text: String,
    /// Why the rule exists — rationale matters more than the text.
    pub rationale: String,
    /// Where the rule applies. Empty = global.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
    /// Free-form tags for later filtering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Origin: e.g. "user via agent, 2026-03-01".
    pub origin: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
    /// Environment the rule applies to ("dev", "prod", ...). None = all.
    #[serde(default)]
    pub env: Option<String>,
    /// Bind the rule to an executable check id (from BUILTIN_CHECKS): the
    /// check's violations carry the rule as their documented "why".
    #[serde(default)]
    pub check: Option<String>,
    /// "active" (default) | "deprecated" — deprecated rules stay in the file
    /// (visible, visibly dead) until removed. None = active.
    #[serde(default)]
    pub status: Option<String>,
}

impl Rule {
    /// Does this rule apply to the environment the session declared?
    /// A rule without `env` applies everywhere; an env-specific rule only
    /// shows up when the session declares that env.
    pub fn matches_env(&self, env: Option<&str>) -> bool {
        match self.env.as_deref() {
            None => true,
            Some(rule_env) => env.map_or(false, |e| e.eq_ignore_ascii_case(rule_env)),
        }
    }
}

impl Rule {
    pub fn is_deprecated(&self) -> bool {
        self.status.as_deref() == Some("deprecated")
    }
}

impl Rules {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Rules that apply to a given scope path (rule is global, or scope matches exactly).
    pub fn for_scope(&self, scope: &str) -> Vec<&Rule> {
        self.rules
            .iter()
            .filter(|r| r.scope.is_empty() || r.scope.iter().any(|s| scope.starts_with(s)))
            .collect()
    }

    /// Mark a rule deprecated (visibly dead, still present).
    pub fn deprecate(&mut self, id: &str, reason: &str) -> anyhow::Result<()> {
        let rule = self
            .rules
            .iter_mut()
            .find(|r| r.id == id)
            .with_context(|| format!("rule `{id}` not found"))?;
        rule.status = Some("deprecated".into());
        if !reason.is_empty() {
            rule.rationale = format!("{} Deprecated: {}", rule.rationale, reason);
        }
        Ok(())
    }

    /// Bring a deprecated rule back.
    pub fn activate(&mut self, id: &str) -> anyhow::Result<()> {
        let rule = self
            .rules
            .iter_mut()
            .find(|r| r.id == id)
            .with_context(|| format!("rule `{id}` not found"))?;
        rule.status = None;
        Ok(())
    }

    /// Remove a rule outright (the historical record lives in git).
    pub fn remove(&mut self, id: &str) -> anyhow::Result<()> {
        let before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        if self.rules.len() == before {
            anyhow::bail!("rule `{id}` not found");
        }
        Ok(())
    }

    /// Compact, token-efficient rendering for agent injection.
    ///
    /// `env` filters environment-specific rules: rules without `env` always
    /// show; env-tagged rules only show when the session declares that env.
    pub fn render_for_agent(&self, scope: Option<&str>, env: Option<&str>) -> String {
        let matched: Vec<&Rule> = match scope {
            Some(s) => self.for_scope(s),
            None => self.rules.iter().collect(),
        };
        let matched: Vec<&Rule> = matched.into_iter().filter(|r| r.matches_env(env)).collect();
        if matched.is_empty() {
            return "No rules recorded for this scope yet.".into();
        }
        let mut out = String::new();
        for r in matched {
            let status = if r.is_deprecated() { " (DEPRECATED — do not follow)" } else { "" };
            out.push_str(&format!("- [{}]{} {}\n", r.id, status, r.text));
            out.push_str(&format!("  why: {}\n", r.rationale));
            if !r.scope.is_empty() {
                out.push_str(&format!("  scope: {}\n", r.scope.join(", ")));
            }
            if let Some(rule_env) = &r.env {
                out.push_str(&format!("  env: {rule_env}\n"));
            }
            if let Some(check) = &r.check {
                out.push_str(&format!("  enforced by check: {check}\n"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Rules {
        let toml_src = r#"
[[rules]]
id = "no-hardcoded-urls"
text = "Use `Endpoint` constants; never hardcode URLs"
rationale = "Hardcoded URLs broke staging deploys twice"
scope = ["src/api"]
tags = ["api"]
origin = "user via agent, 2026-03-01"
examples = ["let url = Endpoint::Users;"]

[[rules]]
id = "transiters-for-engines"
text = "Engines never call each other directly; route through transiters"
rationale = "Direct coupling made the billing engine unreusable"
scope = ["src/engine"]
origin = "user via agent, 2026-03-02"
"#;
        toml::from_str(toml_src).unwrap()
    }

    #[test]
    fn scope_filtering() {
        let rules = sample();
        assert_eq!(rules.for_scope("src/api/handlers").len(), 1);
        assert_eq!(rules.for_scope("src/api").len(), 1);
        assert_eq!(rules.for_scope("src/engine").len(), 1);
        assert_eq!(rules.for_scope("src/other").len(), 0);
    }

    #[test]
    fn render_is_token_efficient() {
        let rules = sample();
        let rendered = rules.render_for_agent(Some("src/api/handlers"), None);
        assert!(rendered.contains("no-hardcoded-urls"));
        assert!(!rendered.contains("transiters-for-engines"));
        // rationale is included so agents understand, not just obey
        assert!(rendered.contains("why:"));
    }

    #[test]
    fn missing_schema_version_defaults_to_current() {
        // Old files written before schema_version existed must still load.
        let rules: Rules = toml::from_str("# comment only").unwrap();
        assert_eq!(rules.schema_version, crate::SCHEMA_VERSION);
    }

    fn env_sample() -> Rules {
        let toml_src = r#"
[[rules]]
id = "verbose-logging"
text = "Log at trace level in handlers"
rationale = "Debugging onboarding flows"
scope = ["src/api"]
origin = "user via agent"
env = "dev"

[[rules]]
id = "no-panics"
text = "Return Result always; never panic"
rationale = "Service restarts in prod cost real money"
scope = ["src/api"]
origin = "user via agent"

[[rules]]
id = "mock-stripe"
text = "Stripe calls go through the mock adapter"
rationale = "No real charges from dev sessions"
scope = ["src/api"]
origin = "user via agent"
env = "dev"
"#;
        toml::from_str(toml_src).unwrap()
    }

    #[test]
    fn env_scoped_rules_only_show_for_declared_env() {
        let rules = env_sample();

        // Session declares dev: global + dev-specific rules.
        let dev = rules.render_for_agent(Some("src/api"), Some("dev"));
        assert!(dev.contains("no-panics"));
        assert!(dev.contains("verbose-logging"));
        assert!(dev.contains("mock-stripe"));

        // Session declares no env: only env-free rules.
        let none = rules.render_for_agent(Some("src/api"), None);
        assert!(none.contains("no-panics"));
        assert!(!none.contains("verbose-logging"));
        assert!(!none.contains("mock-stripe"));

        // Session declares prod: still no dev rules.
        let prod = rules.render_for_agent(Some("src/api"), Some("prod"));
        assert!(prod.contains("no-panics"));
        assert!(!prod.contains("mock-stripe"));
    }

    #[test]
    fn env_matching_is_case_insensitive() {
        let rules = env_sample();
        assert!(rules.rules[0].matches_env(Some("DEV")));
    }
}
