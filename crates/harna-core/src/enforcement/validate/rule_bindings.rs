//! Component `rule-bindings`: coherence between rules.toml and the check
//! registry — the seam where documentation (why) meets enforcement (what).
//! The registry (`enforcement/checks.rs`, backed by checks.toml) is the one
//! canonical source of check semantics; this component reads nothing else.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::model::rules::Rules;

/// Every active `check = "..."` rule binding: check id -> bound rule ids.
/// Deprecated rules are dead and contribute nothing (not even bindings).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bindings(BTreeMap<String, Vec<String>>);

impl Bindings {
    /// Active rule bindings **that apply to this environment**: env-tagged
    /// rules for other environments stay out of enforcement (policy as
    /// data — the same rule set relaxes or intensifies per env). Pattern
    /// rules bind their `pattern` check implicitly.
    pub fn from_rules(rules: &Rules, env: Option<&str>) -> Bindings {
        let mut map = BTreeMap::new();
        for r in &rules.rules {
            if r.is_deprecated() || !r.matches_env(env) {
                continue;
            }
            if let Some(check) = &r.check {
                map.entry(check.clone())
                    .or_insert_with(Vec::new)
                    .push(r.id.clone());
            }
            if r.pattern.is_some() && !r.check.as_deref().is_some_and(checks::takes_pattern) {
                map.entry("pattern".to_string())
                    .or_insert_with(Vec::new)
                    .push(r.id.clone());
            }
        }
        Bindings(map)
    }

    /// Whether a live rule binds `check` — explicitly, or by carrying the
    /// check's own id as its rule id (the natural way to adopt one).
    pub fn binds(&self, rules: &Rules, env: Option<&str>, check: &str) -> bool {
        self.0.contains_key(check)
            || rules
                .rules
                .iter()
                .any(|r| r.id == check && !r.is_deprecated() && r.matches_env(env))
    }

    /// Unknown bindings are a rules-file warning: a typo'd check id means
    /// the rule is documentation-only with nobody enforcing it.
    pub fn check_unknown_bindings(&self, out: &mut Vec<Violation>) {
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
        env: Option<&str>,
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
            if r.is_deprecated() || !r.matches_env(env) {
                continue;
            }
            // A pattern rule binds the `pattern` check implicitly.
            let eff_check = r
                .check
                .as_deref()
                .or_else(|| r.pattern.as_ref().map(|_| "pattern"));
            match eff_check {
                None => {
                    out.push(violation(
                        format!(
                            "rule `{}` sets severity `{sev_src}` but binds no check",
                            r.id
                        ),
                        "severity overrides apply to a bound check — add `check = \"<id>\"`, `pattern = \"...\"` or drop `severity`".to_string(),
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
                // a finding that names its rule is that rule's alone (1.0.1,
                // proof: an f64 finding also listed an unrelated rule bound
                // to the same check)
                let named: Vec<&String> = bound
                    .iter()
                    .filter(|id| v.message.contains(&format!("(rule `{id}`)")))
                    .collect();
                let shown: Vec<&String> = if named.is_empty() {
                    bound.iter().collect()
                } else {
                    named
                };
                let shown: Vec<&str> = shown.iter().map(|s| s.as_str()).collect();
                v.hint
                    .push_str(&format!("  [bound rule(s): {}]", shown.join(", ")));
            }
        }
    }

    /// A `limit` only means something on a budget check (0.3.43): on any
    /// other check — or with no check — it is a number nothing reads, and
    /// a hand-edited rules.toml would believe it enforced.
    pub fn check_limit_coherence(
        &self,
        rules: &Rules,
        env: Option<&str>,
        out: &mut Vec<Violation>,
    ) {
        for r in &rules.rules {
            if r.is_deprecated() || !r.matches_env(env) || r.limit.is_none() {
                continue;
            }
            let check = r.check.as_deref().unwrap_or("");
            if checks::takes_limit(check) {
                continue;
            }
            let what = if check.is_empty() {
                "binds no check".to_string()
            } else {
                format!("binds `{check}`, which takes no limit")
            };
            out.push(Violation {
                check: "manifest-depends".into(),
                file: ".harna/rules.toml".into(),
                line: 0,
                message: format!(
                    "rule `{}` sets limit {} but {what} — nothing reads it",
                    r.id,
                    r.limit.unwrap_or(0)
                ),
                hint: format!(
                    "bind one of the budget checks ({}) or drop `limit`",
                    checks::limit_ids_listed()
                ),
                severity: Severity::Info,
            });
        }
    }

    /// A rule that carries a `pattern` executes the `pattern` check; an
    /// explicit `check` on the same rule must agree (or be absent).
    /// Hand-edited rules.toml stays visible — never silently half-honored.
    pub fn check_pattern_coherence(
        &self,
        rules: &Rules,
        env: Option<&str>,
        out: &mut Vec<Violation>,
    ) {
        for r in &rules.rules {
            if r.is_deprecated() || !r.matches_env(env) {
                continue;
            }
            let (Some(_pat), Some(c)) = (&r.pattern, r.check.as_ref()) else {
                continue;
            };
            if c != "pattern" && !checks::takes_pattern(c) {
                out.push(Violation {
                    check: "manifest-depends".into(),
                    file: ".harna/rules.toml".into(),
                    line: 0,
                    message: format!(
                        "rule `{}` carries a regex pattern but binds check `{c}` — pattern rules run the `pattern` check",
                        r.id
                    ),
                    hint: "drop the explicit `check` or change it to `check = \"pattern\"`".into(),
                    severity: Severity::Info,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_src(check: Option<&str>, severity: Option<&str>, status: Option<&str>) -> String {
        let check_line = check
            .map(|c| format!("check = \"{c}\""))
            .unwrap_or_default();
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

    /// 1.0.1: a finding that names its rule lists that rule alone.
    #[test]
    fn a_finding_that_names_its_rule_is_annotated_with_it_alone() {
        let rules: Rules = toml::from_str(
            "[[rules]]\nid = \"money\"\ntext = \"t\"\nrationale = \"r\"\ncheck = \"code-pattern\"\n\
             [[rules]]\nid = \"log\"\ntext = \"t\"\nrationale = \"r\"\ncheck = \"code-pattern\"\n",
        )
        .unwrap();
        let b = Bindings::from_rules(&rules, None);
        let mut vs = vec![violation_for("code-pattern", Severity::Error)];
        vs[0].message = "f64 found (rule `money`)".into();
        b.annotate(&mut vs);
        assert!(
            vs[0].hint.contains("[bound rule(s): money]"),
            "{}",
            vs[0].hint
        );
        let mut plain = vec![violation_for("code-pattern", Severity::Error)];
        b.annotate(&mut plain);
        assert!(plain[0].hint.contains("money, log"), "{}", plain[0].hint);
    }

    #[test]
    fn unknown_bindings_warn_known_annotate() {
        // Known check: binds cleanly.
        let rules: Rules =
            toml::from_str(&rule_src(Some("no-hardcoded-urls"), None, None)).unwrap();
        let b = Bindings::from_rules(&rules, None);
        let mut v = Vec::new();
        b.check_unknown_bindings(&mut v);
        assert!(v.is_empty());
        let mut vs = vec![violation_for("no-hardcoded-urls", Severity::Error)];
        b.annotate(&mut vs);
        assert!(vs[0].hint.contains("[bound rule(s): r1]"));

        // Unknown check: warning, no annotation crash.
        let rules: Rules = toml::from_str(&rule_src(Some("no-such-check"), None, None)).unwrap();
        let b2 = Bindings::from_rules(&rules, None);
        let mut v2 = Vec::new();
        b2.check_unknown_bindings(&mut v2);
        assert_eq!(v2.len(), 1);
        assert_eq!(v2[0].severity, Severity::Info);
        assert!(v2[0].hint.contains("no-hardcoded-urls"));
        b2.annotate(&mut vs);
    }

    #[test]
    fn deprecated_rules_bind_and_override_nothing() {
        let rules: Rules = toml::from_str(&rule_src(
            Some("layer-boundary"),
            Some("info"),
            Some("deprecated"),
        ))
        .unwrap();
        let b = Bindings::from_rules(&rules, None);
        let (overrides, v) = b.resolve_severity_overrides(&rules, None);
        assert!(
            overrides.is_empty(),
            "a dead rule cannot change enforcement"
        );
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
        let (overrides, v) =
            Bindings::from_rules(&rules, None).resolve_severity_overrides(&rules, None);
        assert!(v.is_empty());
        assert_eq!(overrides.get("layer-boundary"), Some(&Severity::Info));

        let mut vs = [
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
        let src = "schema_version = 1\n\n[[rules]]\nid = \"a\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\ncheck = \"layer-boundary\"\nseverity = \"info\"\n\n[[rules]]\nid = \"b\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\ncheck = \"layer-boundary\"\nseverity = \"error\"\n".to_string();
        let rules: Rules = toml::from_str(&src).unwrap();
        let (overrides, v) =
            Bindings::from_rules(&rules, None).resolve_severity_overrides(&rules, None);
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
        let (overrides, v) =
            Bindings::from_rules(&rules, None).resolve_severity_overrides(&rules, None);
        assert!(overrides.is_empty());
        assert_eq!(v.len(), 1);
        assert!(v[0].hint.contains("error | info"));

        // Severity without a bound check.
        let rules: Rules = toml::from_str(&rule_src(None, Some("info"), None)).unwrap();
        let (overrides, v) =
            Bindings::from_rules(&rules, None).resolve_severity_overrides(&rules, None);
        assert!(overrides.is_empty());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("binds no check"));
    }

    #[test]
    fn env_tagged_rules_stay_out_of_other_environments() {
        let src = "schema_version = 1\n\n[[rules]]\nid = \"dev-rule\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\nenv = \"dev\"\ncheck = \"layer-boundary\"\nseverity = \"info\"\n".to_string();
        let rules: Rules = toml::from_str(&src).unwrap();

        // Declared dev: the dev rule participates.
        let b_dev = Bindings::from_rules(&rules, Some("dev"));
        assert!(b_dev.0.contains_key("layer-boundary"));
        let (overrides, _) = b_dev.resolve_severity_overrides(&rules, Some("dev"));
        assert_eq!(overrides.get("layer-boundary"), Some(&Severity::Info));

        // Declared prod (or nothing): the dev rule is out of enforcement.
        let b_prod = Bindings::from_rules(&rules, Some("prod"));
        assert!(b_prod.0.is_empty());
        let (overrides_prod, _) = b_prod.resolve_severity_overrides(&rules, Some("prod"));
        assert!(overrides_prod.is_empty());
        let b_none = Bindings::from_rules(&rules, None);
        assert!(b_none.0.is_empty());
    }

    #[test]
    fn pattern_rules_bind_the_pattern_check_and_coherence_enforces_agreement() {
        let src = "schema_version = 1\n\n[[rules]]\nid = \"naming\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\npattern = \"banana\"\n\n[[rules]]\nid = \"confused\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\ncheck = \"layer-boundary\"\npattern = \"todo!\"\n";
        let rules: Rules = toml::from_str(src).unwrap();
        let b = Bindings::from_rules(&rules, None);
        assert_eq!(
            b.0.get("pattern").map(|v| v.len()),
            Some(2),
            "both pattern rules bind pattern"
        );
        assert!(
            b.0.contains_key("layer-boundary"),
            "an explicit `check` remains bound even when a pattern rides along"
        );

        let mut v = Vec::new();
        b.check_pattern_coherence(&rules, None, &mut v);
        assert_eq!(v.len(), 1, "the explicit check must agree with the pattern");
        assert!(v[0].message.contains("confused"));
        assert!(v[0].message.contains("`pattern` check"));
    }

    /// ledgerly 11/14 (0.3.43): a `limit` nothing reads is named; the
    /// advisory thresholds and transiter-thinness now take one.
    #[test]
    fn a_limit_on_a_check_that_reads_none_is_named() {
        let rule = |id: &str, check: Option<&str>| crate::schema::model::rules::Rule {
            id: id.into(),
            text: "t".into(),
            rationale: "r".into(),
            origin: "o".into(),
            check: check.map(str::to_string),
            limit: Some(10),
            ..Default::default()
        };
        let mut rules = Rules::default();
        for (id, check) in [
            ("granularity", Some("component-granularity")),
            ("heat", Some("hotspot")),
            ("thin", Some("transiter-thinness")),
            ("cycles", Some("no-cycles")),
            ("loose", None),
        ] {
            rules.rules.push(rule(id, check));
        }
        let b = Bindings::from_rules(&rules, None);
        let mut v = Vec::new();
        b.check_limit_coherence(&rules, None, &mut v);
        let named: Vec<&str> = v
            .iter()
            .map(|x| x.message.split('`').nth(1).unwrap_or(""))
            .collect();
        assert_eq!(named, vec!["cycles", "loose"], "{v:?}");
        assert!(v[0].message.contains("which takes no limit"));
        assert!(v[1].message.contains("binds no check"));
        assert!(v[0].hint.contains("component-granularity"));
    }
}
