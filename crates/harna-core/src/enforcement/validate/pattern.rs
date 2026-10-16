//! Component `pattern-rules`: naming conventions as lint-as-data.
//!
//! A rule's `pattern` regex is matched over file paths and declared names
//! (pub and private) during validate — the same machinery the digest
//! inventory uses, so a name the map catalogs is exactly a name the
//! pattern can police. The rule supplies the regex, the scope-why (its
//! rationale becomes the violation's hint) and optionally a severity
//! override; the registry supplies the semantics.

use regex::Regex;

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::rules::Rules;

/// One active pattern rule, compiled once per validate run.
#[derive(Debug, Clone)]
pub struct ActivePattern {
    pub rule_id: String,
    pub exercise: String,
    pub rationale: String,
    pub re: Regex,
    pub severity: Severity,
}

/// Compile the env-applicable pattern rules. Hand-edited rules with a bad
/// regex are a rules-file violation, never a panic — the rest of the run
/// keeps going.
pub fn compile(
    rules: &Rules,
    env: Option<&str>,
    out: &mut Vec<Violation>,
) -> Vec<ActivePattern> {
    let mut ready = Vec::new();
    for r in &rules.rules {
        if r.is_deprecated() || !r.matches_env(env) {
            continue;
        }
        let Some(pat) = &r.pattern else { continue };
        match Regex::new(pat) {
            Ok(re) => ready.push(ActivePattern {
                rule_id: r.id.clone(),
                exercise: r.text.clone(),
                rationale: r.rationale.clone(),
                re,
                severity: default_severity(r.severity.as_deref()),
            }),
            Err(e) => out.push(Violation {
                check: "manifest-depends".into(),
                file: ".harna/rules.toml".into(),
                line: 0,
                message: format!(
                    "rule `{}` has an un-compilable pattern `{pat}`: {e}",
                    r.id
                ),
                hint: "fix the regex (syntax is the Rust regex crate) or drop the pattern".into(),
                severity: Severity::Info,
            }),
        }
    }
    ready
}

fn default_severity(override_src: Option<&str>) -> Severity {
    override_src
        .and_then(checks::parse_severity)
        .unwrap_or(Severity::Info)
}

/// Apply the compiled patterns to one file: its relative path and every
/// declared name it carries (the same name list the inventory catalogs).
pub fn apply(
    patterns: &[ActivePattern],
    file: &str,
    declared_names: &[String],
    out: &mut Vec<Violation>,
) {
    if patterns.is_empty() {
        return;
    }
    let mut hits: Vec<(&ActivePattern, String)> = Vec::new();
    for p in patterns {
        if p.re.is_match(file) {
            hits.push((p, format!("path `{file}`")));
            continue;
        }
        if let Some(name) = declared_names.iter().find(|n| p.re.is_match(n)) {
            hits.push((p, format!("declared name `{name}`")));
        }
    }
    for (p, what) in hits {
        out.push(Violation {
            check: "pattern".into(),
            file: file.to_string(),
            line: 0,
            severity: p.severity,
            message: format!(
                "rule `{}`: {} — pattern matched {what}",
                p.rule_id, p.exercise
            ),
            hint: p.rationale.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::rules::Rule;

    fn rule(id: &str, pattern: &str, env: Option<&str>, severity: Option<&str>) -> Rule {
        Rule {
            id: id.into(),
            text: format!("convention for {id}"),
            rationale: "because it broke once".into(),
            scope: vec![],
            tags: vec![],
            origin: "test".into(),
            examples: vec![],
            env: env.map(|e| e.into()),
            check: None,
            severity: severity.map(|s| s.into()),
            pattern: Some(pattern.into()),
            status: None,
        }
    }

    fn rules() -> Rules {
        Rules { schema_version: 1, rules: vec![] }
    }

    #[test]
    fn pattern_matches_paths_and_declared_names() {
        let mut r = rules();
        // Same rule: hits on path and name, one violation per file.
        r.rules.push(rule("no-banana", "banana", None, None));
        let mut v = Vec::new();
        let ps = compile(&r, None, &mut v);
        assert!(v.is_empty());

        let mut out = Vec::new();
        apply(
            &ps,
            "src/api/banana.rs",
            &["good".to_string(), "banana_split".to_string()],
            &mut out,
        );
        assert_eq!(out.len(), 1, "one rule matched once per file");
        assert_eq!(out[0].check, "pattern");
        assert_eq!(out[0].severity, Severity::Info, "registry default");
        assert_eq!(out[0].file, "src/api/banana.rs");
        assert!(out[0].hint.contains("broke once"), "rationale is the hint");
        // A file can rise through its declared names.
        let mut out2 = Vec::new();
        apply(&ps, "src/api/clean.rs", &[], &mut out2);
        assert!(out2.is_empty());
    }

    #[test]
    fn env_scoped_and_severity_overrides_apply() {
        let mut r = rules();
        r.rules.push(rule("dev-only", "mockstripe", Some("dev"), Some("error")));
        r.rules.push(rule("prod-only", "todo!", Some("prod"), None));

        let mut v = Vec::new();
        let dev = compile(&r, Some("dev"), &mut v);
        assert_eq!(dev.len(), 1);
        assert_eq!(dev[0].severity, Severity::Error, "override honored");

        let mut v2 = Vec::new();
        let none = compile(&r, None, &mut v2);
        assert!(none.is_empty(), "env-tagged rules stay out unless declared");

        let mut v3 = Vec::new();
        let prod = compile(&r, Some("PROD"), &mut v3);
        assert_eq!(prod.len(), 1);
        assert_eq!(prod[0].severity, Severity::Info, "registry default");
    }

    #[test]
    fn bad_regex_is_a_violation_not_a_crash() {
        let mut r = rules();
        r.rules.push(rule("broken", "(unclosed", None, None));
        let mut v = Vec::new();
        let ps = compile(&r, None, &mut v);
        assert!(ps.is_empty());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("un-compilable pattern `(unclosed`"));
        assert_eq!(v[0].severity, Severity::Info);
        // A good rule alongside still compiles.
        r.rules.push(rule("fine", "todo!", None, None));
        let mut v2 = Vec::new();
        assert_eq!(compile(&r, None, &mut v2).len(), 1);
        assert_eq!(v2.len(), 1);
    }

    #[test]
    fn unknown_severity_word_falls_back_to_registry_default() {
        let mut r = rules();
        r.rules.push(rule("odd", "todo!", None, Some("warn")));
        let mut v = Vec::new();
        let ps = compile(&r, None, &mut v);
        assert_eq!(ps[0].severity, Severity::Info);
    }
}
