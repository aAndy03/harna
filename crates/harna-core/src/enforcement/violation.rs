//! Component `violation`: the shared conformance-finding shape.
//!
//! The type lives beside (not inside) `validate` so the check registry,
//! the rule bindings and the validate components can all depend on it
//! without the registry pointing back into the checker (the 0.3.10
//! `no-cycles` dogfood finding: `checks -> validate::Severity` while every
//! validate child consulted the registry read as rings across all of
//! them). Data shape owned by integrity; consumers import it directly.

/// One conformance violation found in the codebase.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Violation {
    /// Which check fired, e.g. "no-hardcoded-urls".
    pub check: String,
    /// File path relative to repo root.
    pub file: String,
    /// 1-based line number, or 0 when not line-specific.
    pub line: usize,
    /// Human/agent-readable description.
    pub message: String,
    /// Suggested fix, rendered for the agent.
    pub hint: String,
    /// Error violations gate commits/CI (exit code 2); info ones advise only.
    #[serde(default)]
    pub severity: Severity,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Error,
    Info,
}

impl Violation {
    /// Token-efficient rendering for agent consumption.
    pub fn render(&self) -> String {
        let marker = match self.severity {
            Severity::Error => "!",
            Severity::Info => "?",
        };
        format!(
            "[{marker}{}] {}:{} — {}\n  fix: {}",
            self.check,
            self.file,
            if self.line > 0 {
                self.line.to_string()
            } else {
                "-".into()
            },
            self.message,
            self.hint
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape pins its additive serde contract: old payloads without a
    /// severity deserialize as Error; render marks the halves distinctly.
    #[test]
    fn severity_defaults_and_render_markers() {
        let raw = r#"{"check":"x-check","file":"a.rs","line":7,"message":"m","hint":"h"}"#;
        let v: Violation = serde_json::from_str(raw).unwrap();
        assert_eq!(v.severity, Severity::Error, "missing severity stays Error");
        assert!(v.render().starts_with("[!") && v.render().contains("a.rs:7"));

        let mut v = v;
        v.severity = Severity::Info;
        assert!(v.render().starts_with("[?"));
        let text = serde_json::to_string(&v).unwrap();
        assert!(text.contains(r#""severity":"info""#), "round-trips as data");
    }
}
