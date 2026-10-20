//! `code-pattern` (0.5.1): a convention over code lines, as data.
//!
//! Plugins carry framework knowledge, and harna never runs a plugin's code
//! (quality `no-code-execution`): a plugin check is a rule binding one of
//! harna's generic engines. This one matches the rule's `pattern` against
//! live code lines (comments and test items masked) and reports each hit.
//! Two optional `values` sharpen it:
//! - `within:<regex>` — only inside the brace block a matching line opens
//!   (`within:fn render\(` → `cx.new(` created inside render);
//! - `unless:<regex>` — a hit whose line also matches is fine
//!   (`unless:\.detach\(\)`).
//!
//! Text, never parse trees: cheap, honest about being a heuristic, and the
//! rule's rationale says what the pattern stands for.

use regex::Regex;

use crate::enforcement::violation::{Severity, Violation};
use crate::schema::model::rules::{Rule, Rules};
use crate::schema::model::scope::path_within;
use crate::search::digest::live::live_code;

struct Bound<'r> {
    rule: &'r Rule,
    pattern: Regex,
    within: Option<Regex>,
    unless: Option<Regex>,
}

/// The rules bound to `code-pattern`, compiled once per validate run.
/// A rule whose regexes do not compile is reported once, as data.
pub fn compile<'r>(
    rules: &'r Rules,
    env: Option<&str>,
    out: &mut Vec<Violation>,
) -> Vec<BoundRule<'r>> {
    let mut bound = Vec::new();
    for rule in rules.rules.iter().filter(|r| {
        !r.is_deprecated() && r.matches_env(env) && r.check.as_deref() == Some("code-pattern")
    }) {
        let mut problem = None;
        let re = |s: &str| Regex::new(s).map_err(|e| e.to_string());
        let pattern = match rule.pattern.as_deref().map(re) {
            Some(Ok(p)) => Some(p),
            Some(Err(e)) => {
                problem = Some(format!("pattern does not compile: {e}"));
                None
            }
            None => {
                problem = Some("code-pattern needs a --pattern".into());
                None
            }
        };
        let mut within = None;
        let mut unless = None;
        for v in &rule.values {
            let (slot, raw) = if let Some(r) = v.strip_prefix("within:") {
                (&mut within, r)
            } else if let Some(r) = v.strip_prefix("unless:") {
                (&mut unless, r)
            } else {
                problem = Some(format!(
                    "value `{v}` is neither within:<regex> nor unless:<regex>"
                ));
                continue;
            };
            match re(raw) {
                Ok(r) => *slot = Some(r),
                Err(e) => problem = Some(format!("value `{v}` does not compile: {e}")),
            }
        }
        match (pattern, problem) {
            (Some(pattern), None) => bound.push(BoundRule(Bound {
                rule,
                pattern,
                within,
                unless,
            })),
            (_, why) => out.push(Violation {
                check: "harna-data".into(),
                file: ".harna/rules.toml".into(),
                line: 0,
                message: format!(
                    "rule `{}`: {} — it checks nothing until fixed",
                    rule.id,
                    why.unwrap_or_default()
                ),
                hint: "fix the rule's pattern/values (harna rules shows it)".into(),
                severity: Severity::Error,
            }),
        }
    }
    bound
}

/// A compiled `code-pattern` rule (opaque to callers).
pub struct BoundRule<'r>(Bound<'r>);

pub fn check(bound: &[BoundRule], file: &str, src: &str, out: &mut Vec<Violation>) {
    let here: Vec<&Bound> = bound
        .iter()
        .map(|b| &b.0)
        .filter(|b| b.rule.scope.is_empty() || b.rule.scope.iter().any(|s| path_within(file, s)))
        .collect();
    if here.is_empty() {
        return;
    }
    let live = live_code(src);
    for b in here {
        // depth of the innermost open `within` block (None = outside)
        let mut open: Vec<i32> = Vec::new();
        let mut depth: i32 = 0;
        for (n, line) in &live {
            let opens_block = b.within.as_ref().is_some_and(|w| w.is_match(line));
            if opens_block {
                open.push(depth);
            }
            let inside = b.within.is_none() || !open.is_empty();
            if inside
                && b.pattern.is_match(line)
                && !b.unless.as_ref().is_some_and(|u| u.is_match(line))
            {
                out.push(Violation {
                    check: "code-pattern".into(),
                    file: file.into(),
                    line: *n,
                    severity: crate::enforcement::checks::severity_of("code-pattern"),
                    message: format!("{} (rule `{}`)", b.rule.text, b.rule.id),
                    hint: b.rule.rationale.clone(),
                });
            }
            depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
            while open.last().is_some_and(|d| depth <= *d && !opens_block) {
                open.pop();
            }
            if opens_block && depth <= *open.last().unwrap_or(&i32::MIN) {
                open.pop(); // a one-line block closed on its own line
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, values: &[&str]) -> Rules {
        let mut r = Rules::default();
        r.rules.push(Rule {
            id: "r".into(),
            text: "t".into(),
            check: Some("code-pattern".into()),
            pattern: Some(pattern.into()),
            values: values.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        });
        r
    }

    fn hits(rules: &Rules, src: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let bound = compile(rules, None, &mut out);
        assert!(out.is_empty(), "{out:?}");
        check(&bound, "src/view.rs", src, &mut out);
        out.iter().map(|v| v.line).collect()
    }

    /// 0.5.1: `cx.new(` inside render is the finding; the same call in
    /// `new()` is not; comments never count.
    #[test]
    fn within_limits_hits_to_the_block() {
        let src = "impl View {\n    fn new(cx: &mut App) -> Self {\n        let s = cx.new(|_| 0);\n    }\n    fn render(&mut self, cx: &mut App) {\n        // cx.new( in a comment\n        let x = cx.new(|_| 1);\n        div()\n    }\n    fn after(&self, cx: &mut App) { cx.new(|_| 2); }\n}\n";
        assert_eq!(
            hits(&rule(r"cx\.new\(", &[r"within:fn render\("]), src),
            vec![7]
        );
        assert_eq!(hits(&rule(r"cx\.new\(", &[]), src).len(), 3);
    }

    #[test]
    fn unless_excuses_a_line_and_bad_rules_are_data_errors() {
        let src = "fn wire(cx: &mut App) {\n    cx.subscribe(&a, f);\n    cx.subscribe(&b, g).detach();\n}\n";
        assert_eq!(
            hits(&rule(r"^cx\.subscribe\(", &[r"unless:\.detach\(\)"]), src),
            vec![2]
        );
        let mut out = Vec::new();
        assert!(compile(&rule("(", &[]), None, &mut out).is_empty());
        assert_eq!(out[0].check, "harna-data");
        let mut out = Vec::new();
        compile(&rule("x", &["around:y"]), None, &mut out);
        assert_eq!(out.len(), 1);
    }

    /// Every shipped plugin's code-pattern rules compile as data.
    #[test]
    fn shipped_plugin_patterns_compile() {
        for pack in crate::generation::structure::plugin::shipped() {
            let mut out = Vec::new();
            let n = compile(&pack.rules(), None, &mut out).len();
            assert!(out.is_empty(), "{}: {out:?}", pack.meta.name);
            assert!(n > 0 || pack.meta.name != "gpui-kit");
        }
    }
}
