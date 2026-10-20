//! Rules: scoped, persistent coding rules with rationale and origin.
//!
//! Every rule records *why* it exists and *who/when* created it, so a fresh
//! agent session understands the rule, not just its text — without asking.

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::schema::model::manifest::current_schema_version;

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
        Self {
            schema_version: crate::SCHEMA_VERSION,
            rules: Vec::new(),
        }
    }
}

/// A single coding rule.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Bind the rule to an executable check id (see the check registry in
    /// `enforcement/checks.toml`): the check's violations carry the rule as
    /// their documented "why".
    #[serde(default)]
    pub check: Option<String>,
    /// Severity override for the bound check: "error" | "info". A check
    /// earlier bound by severity-carrying rules gates (or advises) at the
    /// overridden level instead of its registry default. Requires `check`.
    #[serde(default)]
    pub severity: Option<String>,
    /// Regex pattern rule: when set, the rule's `pattern` is matched over
    /// file paths and declared names during validate (check `pattern`) —
    /// naming conventions become lint-as-data. Implies binding to the
    /// `pattern` check (an explicit `check` must agree or be absent).
    #[serde(default)]
    pub pattern: Option<String>,
    /// Numeric threshold for budget checks (0.3.7): a rule bound to
    /// `container-size` (components per container) or `fan-out-cap`
    /// (imports per component) carries its budget in `limit`. The
    /// strictest (lowest) applicable limit wins; rules without a limit
    /// bind the check only for annotation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Arguments for checks that take a list (0.3.45): the banned paths of
    /// a `banned-api` rule, the sites of a `sync-group`, … — what each
    /// check reads is in its registry description (`takes_values`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// The command that proves the rule (0.3.48, R3): for `generated`, the
    /// regenerate-and-diff check (`cargo xtask workflows --check`). harna
    /// runs no code; its generated CI workflow runs this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Scope by meaning (0.4.8, R6): a pattern over declarations — the rule
    /// applies to every file declaring a match (`\bMoney\b`), intersected
    /// with `scope` when that is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_signature: Option<String>,
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
            Some(rule_env) => env.is_some_and(|e| e.eq_ignore_ascii_case(rule_env)),
        }
    }
}

impl Rule {
    pub fn is_deprecated(&self) -> bool {
        self.status.as_deref() == Some("deprecated")
    }
}

/// One `values` entry that may name its sanctioned replacement (0.3.48,
/// codex/zed/ruff study R1): `std::process::Command::spawn =>
/// smol::process::Command::spawn` -> (banned, Some(replacement)). zed's
/// clippy.toml and ruff's `System::read_to_string` both say where to go,
/// not only what is gone.
pub fn value_and_replacement(v: &str) -> (&str, Option<&str>) {
    match v.split_once("=>") {
        Some((path, to)) if !to.trim().is_empty() => (path.trim(), Some(to.trim())),
        Some((path, _)) => (path.trim(), None),
        None => (v.trim(), None),
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
            .filter(|r| {
                r.scope.is_empty()
                    || r.scope
                        .iter()
                        .any(|s| crate::schema::model::scope::path_within(scope, s))
            })
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
            let status = if r.is_deprecated() {
                " (DEPRECATED — do not follow)"
            } else {
                ""
            };
            out.push_str(&format!("- [{}]{} {}\n", r.id, status, r.text));
            out.push_str(&format!("  why: {}\n", r.rationale));
            if !r.scope.is_empty() {
                out.push_str(&format!("  scope: {}\n", r.scope.join(", ")));
            }
            if let Some(sig) = &r.scope_signature {
                out.push_str(&format!("  applies to code declaring: {sig}\n"));
            }
            if let Some(rule_env) = &r.env {
                out.push_str(&format!("  env: {rule_env}\n"));
            }
            if let Some(limit) = r.limit {
                out.push_str(&format!("  limit: {limit}\n"));
            }
            if let Some(check) = &r.check {
                out.push_str(&format!("  enforced by check: {check}"));
                if let Some(sev) = &r.severity {
                    out.push_str(&format!(" (severity: {sev})"));
                }
                out.push('\n');
            } else if let Some(pat) = &r.pattern {
                out.push_str(&format!("  pattern (check: pattern): {pat}\n"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_may_name_its_replacement() {
        assert_eq!(
            value_and_replacement("std::process::Command::spawn => smol::process::Command::spawn"),
            (
                "std::process::Command::spawn",
                Some("smol::process::Command::spawn")
            )
        );
        assert_eq!(
            value_and_replacement(" std::fs::read "),
            ("std::fs::read", None)
        );
        assert_eq!(
            value_and_replacement("std::fs::read =>"),
            ("std::fs::read", None)
        );
    }

    #[test]
    fn for_scope_respects_segment_boundaries() {
        let rules: Rules = toml::from_str(
            "[[rules]]\nid = \"api\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"o\"\nscope = [\"src/api\"]\n",
        )
        .unwrap();
        assert_eq!(rules.for_scope("src/api/handlers").len(), 1);
        assert!(rules.for_scope("src/api_v2").is_empty());
    }

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
