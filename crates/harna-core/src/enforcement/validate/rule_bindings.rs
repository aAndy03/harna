//! Component `rule-bindings`: coherence between rules.toml and the check
//! registry — the seam where documentation (why) meets enforcement (what).

use serde::{Deserialize, Serialize};

use super::{Severity, Violation};
use crate::schema::rules::{Rules, BUILTIN_CHECKS};

/// Every `check = "..."` rule binding: check id -> bound rule ids.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bindings(std::collections::BTreeMap<String, Vec<String>>);

impl Bindings {
    pub fn from_rules(rules: &Rules) -> Bindings {
        let mut map = std::collections::BTreeMap::new();
        for r in &rules.rules {
            if let Some(check) = &r.check {
                map.entry(check.clone()).or_insert_with(Vec::new).push(r.id.clone());
            }
        }
        Bindings(map)
    }

    /// Unknown bindings are a rules-file warning: a typo'd check id means
    /// the rule is documentation-only with nobody enforcing it.
    pub fn check_unknown_bindings(
        &self,
        out: &mut Vec<Violation>,
    ) {
        for (check, rules) in &self.0 {
            if !BUILTIN_CHECKS.contains(&check.as_str()) {
                out.push(Violation {
                    check: "manifest-depends".into(),
                    file: ".harna/rules.toml".into(),
                    line: 0,
                    message: format!(
                        "rule(s) {} bind check `{check}` which no validator knows about",
                        rules.join(", ")
                    ),
                    hint: format!(
                        "pick a known check: {}",
                        BUILTIN_CHECKS.join(", ")
                    ),
                    severity: Severity::Info,
                });
            }
        }
    }

    /// Annotate violations with the rule(s) documenting that check — so the
    /// agent reading a violation also gets its rationale.
    pub fn annotate(&self, violations: &mut [Violation]) {
        for v in violations.iter_mut() {
            if let Some(bound) = self.0.get(&v.check) {
                v.hint
                    .push_str(&format!("  [bound rule(s): {}]", bound.join(", ")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_src(check: Option<&str>) -> String {
        let check_line = check
            .map(|c| format!("check = \"{c}\""))
            .unwrap_or_default();
        format!(
            "schema_version = 1\n\n[[rules]]\nid = \"r1\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\n{check_line}\n"
        )
    }

    #[test]
    fn unknown_bindings_warn_known_annotate() {
        use crate::schema::rules::Rules;

        // Known check: binds cleanly.
        let rules: Rules = toml::from_str(&rule_src(Some("no-hardcoded-urls"))).unwrap();
        let b = Bindings::from_rules(&rules);
        let mut v = Vec::new();
        b.check_unknown_bindings(&mut v);
        assert!(v.is_empty());
        let mut vs = vec![Violation {
            check: "no-hardcoded-urls".into(),
            file: "x.rs".into(),
            line: 1,
            message: "m".into(),
            hint: "h".into(),
            severity: Severity::Error,
        }];
        b.annotate(&mut vs);
        assert!(vs[0].hint.contains("[bound rule(s): r1]"));

        // Unknown check: warning, no annotation crash.
        let rules: Rules = toml::from_str(&rule_src(Some("no-such-check"))).unwrap();
        let b2 = Bindings::from_rules(&rules);
        let mut v2 = Vec::new();
        b2.check_unknown_bindings(&mut v2);
        assert_eq!(v2.len(), 1);
        assert_eq!(v2[0].severity, Severity::Info);
        b2.annotate(&mut vs);
    }
}
