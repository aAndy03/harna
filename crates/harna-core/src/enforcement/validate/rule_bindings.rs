//! Component `rule-bindings`: coherence between rules.toml and the check
//! registry — the seam where documentation (why) meets enforcement (what).
//! The registry (`enforcement/checks.rs`, backed by checks.toml) is the one
//! canonical source of check semantics; this component reads nothing else.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::rules::Rules;

/// Every active `check = "..."` rule binding: check id -> bound rule ids.
/// Deprecated rules are dead and contribute nothing (not even bindings).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bindings(BTreeMap<String, Vec<String>>);

impl Bindings {
    pub fn from_rules(rules: &Rules) -> Bindings {
        let mut map = BTreeMap::new();
        for r in &rules.rules {
            if let Some(check) = &r.check {
                if r.is_deprecated() {
                    continue;
                }
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
            if !checks::known(check) {
                out.push(Violation {
                    check: "manifest-depends".into(),
                    file: ".harna/rules.toml".into(),
                    line: 0,
                    message: format!(
                        "rule(s) {} bind check `{check}` which no validator knows about",
                        rules.join(", ")
                    ),
                    hint: format!("pick a known check: {}", checks::known_ids_listed()),
                    severity: Severity::Info,
                });
            }
        }
    }

    /// Resolve severity overrides: every bound rule may set its check's
    /// severity ("error" | "info"), overriding the check's registry default.
    ///
    /// Returns the agreed overrides plus rules-file violations for the
    /// malformed cases so hand-edited files stay visible, not silently
    /// half-honored:
    /// - a rule sets `severity` but binds no check (nothing to override);
    /// - the value is not one of error/info (typo like "warn");
    /// - two bound rules disagree for the same check — the check keeps its
    ///   registry default rather than silently picking a side.
    pub fn resolve_severity_overrides(
        &self,
        rules: &Rules,
    ) -> (BTreeMap<String, Severity>, Vec<Violation>) {
        let mut agreed: BTreeMap<String, Severity> = BTreeMap::new();
        let mut contested: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut out: Vec<Violation> = Vec::new();

        let violation = |message: String, hint: String| Violation {
            check: "manifest-depends".into(),
            file: ".harna/rules.toml".into(),
            line: 0,
            message,
            hint,
            severity: Severity::Info,
        };

        for r in &rules.rules {
            let Some(sev_src) = &r.severity else { continue };
            if r.is_deprecated() {
                continue;
            }
            match r.check.as_deref() {
                None => {
                    out.push(violation(
                        format!(
                            "rule `{}` sets severity `{sev_src}` but binds no check",
                            r.id
                        ),
                        format!(
                            "severity overrides apply to a bound check — add `check = \"<id>\"` or drop `severity`"
                        ),
                    ));
                }
                Some(check) => match checks::parse_severity(sev_src) {
                    Some(sev) => match agreed.get(check) {
                        Some(prev) if *prev != sev && !contested.contains(check) => {
                            contested.insert(check.into());
                            out.push(violation(
                                format!(
                                    "rules `{}` on check `{check}` set conflicting severities ({prev:?} vs {sev:?})",
                                    self.0
                                        .get(check)
                                        .map(|bound| bound.join("`, `"))
                                        .unwrap_or_else(|| r.id.clone()),
                                ),
                                format!(
                                    "delete one override so the check keeps a single severity (registry default: {})",
                                    checks::spec(check)
                                        .map(|s| s.severity_word())
                                        .unwrap_or("info")
                                ),
                            ));
                        }
                        Some(_) if contested.contains(check) => {}
                        Some(_) => {}
                        None => {
                            agreed.insert(check.into(), sev);
                        }
                    },
                    None => {
                        out.push(violation(
                            format!(
                                "rule `{}` sets unknown severity `{sev_src}` for check `{check}`",
                                r.id
                            ),
                            format!("valid values: {}", checks::VALID_SEVERITIES),
                        ));
                    }
                },
            }
        }
        // A contested check keeps its registry default: no override.
        for check in &contested {
            agreed.remove(check);
        }
        (agreed, out)
    }

    /// Annotate violations with the rule(s) documenting that check — so the
    /// agent reading a violation also gets the rule's rationale.
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

    fn rule_src(check: Option<&str>, severity: Option<&str>, status: Option<&str>) -> String {
        let check_line = check.map(|c| format!("check = \"{c}\"")).unwrap_or_default();
        let sev_line = severity
            .map(|s| format!("severity = \"{s}\""))
            .unwrap_or_default();
        let status_line = status
            .map(|s| format!("status = \"{s}\""))
            .unwrap_or_default();
        format!(
            "schema_version = 1\n\n[[rules]]\nid = \"r1\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\n{check_line}\n{sev_line}\n{status_line}\n"
        )
    }

    fn violation_for(check: &str, severity: Severity) -> Violation {
        Violation {
            check: check.into(),
            file: "x.rs".into(),
            line: 1,
            message: "m".into(),
            hint: "h".into(),
            severity,
        }
    }

    #[test]
    fn unknown_bindings_warn_known_annotate() {
        // Known check: binds cleanly.
        let rules: Rules = toml::from_str(&rule_src(Some("no-hardcoded-urls"), None, None)).unwrap();
        let b = Bindings::from_rules(&rules);
        let mut v = Vec::new();
        b.check_unknown_bindings(&mut v);
        assert!(v.is_empty());
        let mut vs = vec![violation_for("no-hardcoded-urls", Severity::Error)];
        b.annotate(&mut vs);
        assert!(vs[0].hint.contains("[bound rule(s): r1]"));

        // Unknown check: warning, no annotation crash.
        let rules: Rules = toml::from_str(&rule_src(Some("no-such-check"), None, None)).unwrap();
        let b2 = Bindings::from_rules(&rules);
        let mut v2 = Vec::new();
        b2.check_unknown_bindings(&mut v2);
        assert_eq!(v2.len(), 1);
        assert_eq!(v2[0].severity, Severity::Info);
        assert!(v2[0].hint.contains("no-hardcoded-urls"));
        b2.annotate(&mut vs);
    }

    #[test]
    fn deprecated_rules_bind_and_override_nothing() {
        let rules: Rules =
            toml::from_str(&rule_src(Some("layer-boundary"), Some("info"), Some("deprecated")))
                .unwrap();
        let b = Bindings::from_rules(&rules);
        let (overrides, v) = b.resolve_severity_overrides(&rules);
        assert!(overrides.is_empty(), "a dead rule cannot change enforcement");
        assert!(v.is_empty());
        let mut unknown = Vec::new();
        b.check_unknown_bindings(&mut unknown);
        assert!(unknown.is_empty());
    }

    #[test]
    fn severity_override_applies_and_gates_follows() {
        // layer-boundary normally errors; a bound rule can demote it to info.
        let rules: Rules =
            toml::from_str(&rule_src(Some("layer-boundary"), Some("info"), None)).unwrap();
        let (overrides, v) = Bindings::from_rules(&rules).resolve_severity_overrides(&rules);
        assert!(v.is_empty());
        assert_eq!(overrides.get("layer-boundary"), Some(&Severity::Info));

        let mut vs = vec![
            violation_for("layer-boundary", Severity::Error),
            violation_for("no-hardcoded-urls", Severity::Error),
        ];
        for viol in vs.iter_mut() {
            if let Some(sev) = overrides.get(&viol.check) {
                viol.severity = *sev;
            }
        }
        assert_eq!(vs[0].severity, Severity::Info);
        assert_eq!(vs[1].severity, Severity::Error, "unbound checks untouched");
    }

    #[test]
    fn conflicting_overrides_keep_the_registry_default() {
        let src = format!(
            "schema_version = 1\n\n[[rules]]\nid = \"a\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\ncheck = \"layer-boundary\"\nseverity = \"info\"\n\n[[rules]]\nid = \"b\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\ncheck = \"layer-boundary\"\nseverity = \"error\"\n"
        );
        let rules: Rules = toml::from_str(&src).unwrap();
        let (overrides, v) = Bindings::from_rules(&rules).resolve_severity_overrides(&rules);
        assert!(
            overrides.is_empty(),
            "a contested check stays at its registry default"
        );
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("conflicting severities"));
        assert!(v[0].message.contains("a`, `b"));
    }

    #[test]
    fn malformed_severity_and_checkless_severity_are_violations() {
        // Typo'd value.
        let rules: Rules =
            toml::from_str(&rule_src(Some("layer-boundary"), Some("warn"), None)).unwrap();
        let (overrides, v) = Bindings::from_rules(&rules).resolve_severity_overrides(&rules);
        assert!(overrides.is_empty());
        assert_eq!(v.len(), 1);
        assert!(v[0].hint.contains("error | info"));

        // Severity without a bound check.
        let rules: Rules = toml::from_str(&rule_src(None, Some("info"), None)).unwrap();
        let (overrides, v) = Bindings::from_rules(&rules).resolve_severity_overrides(&rules);
        assert!(overrides.is_empty());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("binds no check"));
    }
}
