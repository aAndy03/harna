//! `no-panic` — production code in a rule's scope returns errors instead of
//! panicking: `.unwrap()`, `.expect(`, `panic!`, `todo!`, `unimplemented!`.
//!
//! Opt-in by binding (0.3.42, from the font-manager test: the agent wrote a
//! "no unwrap in backend" rule and found nothing could enforce it): the check
//! runs only for files a non-deprecated rule with `check = "no-panic"` covers.
//! Test items, test files and comments are masked (`search::digest::live`); string
//! literal contents are ignored. `unreachable!` stays open — it documents an
//! invariant rather than deferring an error.

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::path_within;

const MARKERS: &[&str] = &[
    ".unwrap()",
    ".expect(",
    "panic!(",
    "todo!(",
    "unimplemented!(",
];

pub fn check(rules: &Rules, env: Option<&str>, file: &str, src: &str, out: &mut Vec<Violation>) {
    let Some(rule) = rules.rules.iter().find(|r| {
        !r.is_deprecated()
            && r.matches_env(env)
            && r.check.as_deref() == Some("no-panic")
            && (r.scope.is_empty() || r.scope.iter().any(|s| path_within(file, s)))
    }) else {
        return;
    };
    for (n, line) in crate::search::digest::live::live_code(src) {
        let code = strip_strings(line);
        let Some(marker) = MARKERS.iter().find(|m| hits(&code, m)) else {
            continue;
        };
        out.push(Violation {
            check: "no-panic".into(),
            file: file.into(),
            line: n,
            severity: checks::spec("no-panic")
                .map(|s| s.default_severity)
                .unwrap_or(Severity::Error),
            message: format!(
                "`{}` can panic in production code (rule `{}`)",
                marker.trim_start_matches('.').trim_end_matches(['(', ')']),
                rule.id
            ),
            hint: "return a typed error (`?`, `ok_or`, `map_err`) or handle the case; \
                   keep unwrap/expect inside tests"
                .into(),
        });
    }
}

/// A macro marker must start a word (`my_panic!(` is someone else's macro).
fn hits(code: &str, marker: &str) -> bool {
    code.match_indices(marker).any(|(at, _)| {
        marker.starts_with('.')
            || code[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
    })
}

/// The line with string-literal contents blanked (`"x.unwrap()"` is text).
fn strip_strings(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_str = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (in_str, c) {
            (true, '\\') => {
                chars.next();
            }
            (true, '"') => {
                in_str = false;
                out.push('"');
            }
            (true, _) => {}
            (false, '"') => {
                in_str = true;
                out.push('"');
            }
            (false, _) => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
