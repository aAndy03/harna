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
        // open `within` blocks: (depth before the opening line, opened?).
        // A block whose `{` comes on a later line (a wrapped signature, a
        // closure argument, 0.5.9) stays pending until the depth rises.
        let mut blocks: Vec<(i32, bool, usize)> = Vec::new();
        let mut depth: i32 = 0;
        for (i, (n, line)) in live.iter().enumerate() {
            if b.within.as_ref().is_some_and(|w| w.is_match(line)) {
                blocks.push((depth, false, i));
            }
            let inside = b.within.is_none() || !blocks.is_empty();
            if inside && b.pattern.is_match(line) && !excused(b, &live, i) {
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
            for blk in blocks.iter_mut() {
                if !blk.1 && depth > blk.0 {
                    blk.1 = true;
                }
            }
            // an opened block closes when the depth falls back; a pending
            // one that never opens (`fn render();`, 8 lines) is dropped
            blocks.retain(|(base, opened, at)| {
                if *opened {
                    depth > *base
                } else {
                    // a block opened and closed on one line is done too
                    let closed_here = line.contains('{') && depth <= *base;
                    !closed_here
                        && !(line.trim_end().ends_with(';') && depth <= *base)
                        && i - at < 8
                }
            });
        }
    }
}

/// `unless:` reads the whole statement the hit starts (0.5.9): rustfmt
/// puts `.detach()` on its own line after a closure argument.
fn excused(b: &Bound, live: &[(usize, &str)], at: usize) -> bool {
    let Some(unless) = &b.unless else {
        return false;
    };
    // the statement ends at a `;` outside every bracket it opened
    // — or it is no statement at all: an element of an enclosing list or
    // struct (`vec![cx.subscribe(…), …]`, `field: cx.subscribe(…),`) is
    // a value stored, which is what an unless-rule asks for (0.5.9)
    let mut stmt = String::new();
    let mut depth: i32 = 0;
    for (_, l) in live.iter().skip(at).take(40) {
        stmt.push_str(l);
        stmt.push(' ');
        for c in l.chars() {
            match c {
                '(' | '{' | '[' => depth += 1,
                ')' | '}' | ']' => {
                    depth -= 1;
                    if depth < 0 {
                        return true; // the enclosing bracket closed: an element
                    }
                }
                ',' if depth == 0 => return true,
                _ => {}
            }
        }
        if depth <= 0 && l.trim_end().ends_with(';') {
            break;
        }
    }
    unless.is_match(&stmt)
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

    /// 0.5.7: the gpui-kit entity rules on real shapes — state reached
    /// through a RefCell inside a Render impl, an index id (bare or in a
    /// tuple), a dropped subscription; their good twins pass.
    #[test]
    fn the_gpui_kit_entity_rules_read_real_shapes() {
        let pack = crate::generation::structure::plugin::resolve("gpui-kit", None).unwrap();
        let rules = pack.rules();
        let mut out = Vec::new();
        let bound = compile(&rules, None, &mut out);
        let src = concat!(
            "struct List { items: Rc<RefCell<Vec<Row>>> }\n",
            "impl Render for List {\n",
            "    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {\n",
            "        let rows = self.items.borrow_mut();\n",
            "        v_flex().children(rows.iter().enumerate().map(|(ix, r)| div().id(ix)))\n",
            "            .child(div().id((\"row\", ix)))\n",
            "            .child(div().id(r.id))\n",
            "    }\n",
            "}\n",
            "impl List {\n",
            "    fn new(cx: &mut Context<Self>) -> Self {\n",
            "        cx.subscribe(&a, f);\n",
            "        let _s = cx.subscribe(&b, g);\n",
            "        cx.observe(&c, h).detach();\n",
            "        Self { items: Rc::new(RefCell::new(vec![])) }\n",
            "    }\n",
            "}\n",
        );
        check(&bound, "crates/feature/src/list.rs", src, &mut out);
        let mut hits: Vec<(usize, String)> = out
            .iter()
            .map(|v| {
                (
                    v.line,
                    v.message.split("(rule `").nth(1).unwrap_or("").to_string(),
                )
            })
            .collect();
        hits.sort();
        let rule_at = |l: usize| {
            hits.iter()
                .filter(|(n, _)| *n == l)
                .map(|(_, r)| r.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        assert!(rule_at(4).contains("state-in-entities"), "{hits:?}");
        assert!(rule_at(5).contains("stable-identity"), "{hits:?}");
        assert!(rule_at(6).contains("stable-identity"), "{hits:?}");
        assert!(rule_at(7).is_empty(), "a domain id is stable: {hits:?}");
        assert!(rule_at(12).contains("subscriptions-are-owned"), "{hits:?}");
        assert!(rule_at(13).is_empty() && rule_at(14).is_empty(), "{hits:?}");
        assert!(
            rule_at(15).is_empty(),
            "RefCell outside Render is not this rule's business: {hits:?}"
        );
    }

    /// 0.5.9 review M5/M6: rustfmt shapes — a wrapped `fn render(` whose
    /// `{` comes later, a dialog builder closure on its own line, a
    /// one-line render, and `.detach()` on the line after a closure.
    #[test]
    fn rustfmt_shapes_are_read_whole() {
        let render = concat!(
            "impl Render for V {\n",
            "    fn render(\n",
            "        &mut self,\n",
            "        cx: &mut Context<Self>,\n",
            "    ) -> impl IntoElement {\n",
            "        let s = cx.new(|_| 0);\n",
            "        div()\n",
            "    }\n",
            "}\n",
            "fn render_once(cx: &mut App) -> Div { div() }\n",
            "fn other(cx: &mut App) {\n",
            "    let t = cx.new(|_| 1);\n",
            "}\n",
        );
        assert_eq!(
            hits(&rule(r"cx\.new\(", &[r"within:fn render\("]), render),
            vec![6]
        );
        let dialog = concat!(
            "fn open(window: &mut Window, cx: &mut App) {\n",
            "    window.open_dialog(\n",
            "        cx,\n",
            "        move |dialog, _, cx| {\n",
            "            let input = cx.new(|_| 0);\n",
            "            dialog\n",
            "        },\n",
            "    );\n",
            "    let ok = cx.new(|_| 1);\n",
            "}\n",
        );
        assert_eq!(
            hits(&rule(r"cx\.new\(", &[r"within:open_dialog\("]), dialog),
            vec![5]
        );
        let sub = concat!(
            "fn wire(cx: &mut App) {\n",
            "    cx.subscribe(&a, |this, _, e, cx| {\n",
            "        this.on(e);\n",
            "    })\n",
            "    .detach();\n",
            "    cx.subscribe(&b, f);\n",
            "}\n",
        );
        assert_eq!(
            hits(&rule(r"^cx\.subscribe\(", &[r"unless:\.detach\(\)"]), sub),
            vec![6]
        );
    }
}
