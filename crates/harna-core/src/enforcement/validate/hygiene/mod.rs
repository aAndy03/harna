//! Checks group `hygiene` (0.3.46, deno study E1–E4, C2, D2, F1): repo
//! hygiene as rule data — the checks deno keeps in `tools/lint.js` by hand.
//!
//! Folder allowlists, file headers, referenced-by (orphan fixtures),
//! ignore-with-reason, count pins, generated files, and claims in agent
//! docs. Each is bound by a rule (its `scope`, `values`, `pattern`,
//! `limit`), except `doc-claims`, which reads the docs agents follow.

use std::path::Path;

use super::Violation;
use crate::schema::model::rules::{Rule, Rules};

pub mod count_pins;
pub mod doc_claims;
pub mod folder_entries;
pub mod generated;
pub mod headers;
pub mod ignores;
pub mod referenced;

/// Every hygiene check, one call from validate.
pub fn check(
    root: &Path,
    rules: &Rules,
    env: Option<&str>,
    digest: Option<&crate::search::digest::Digest>,
    out: &mut Vec<Violation>,
) {
    folder_entries::check(root, rules, env, out);
    headers::check(root, rules, env, out);
    referenced::check(root, rules, env, out);
    ignores::check(root, out);
    count_pins::check(root, rules, env, digest, out);
    generated::check(root, rules, env, out);
    doc_claims::check(root, out);
}

/// Live rules bound to `check`.
pub(crate) fn bound<'r>(rules: &'r Rules, env: Option<&str>, check: &str) -> Vec<&'r Rule> {
    rules
        .rules
        .iter()
        .filter(|r| !r.is_deprecated() && r.matches_env(env) && r.check.as_deref() == Some(check))
        .collect()
}

/// A finding with the registry severity, citing its rule.
pub(crate) fn finding(
    check: &str,
    file: &str,
    line: usize,
    message: String,
    hint: String,
) -> Violation {
    Violation {
        check: check.into(),
        file: file.into(),
        line,
        severity: crate::enforcement::checks::severity_of(check),
        message,
        hint,
    }
}

/// Files under `scope` (repo-relative; empty or `.` = all), git's ignores
/// honored.
pub(crate) fn files_in(root: &Path, scope: &str) -> Vec<String> {
    let scope = crate::schema::model::scope::normalize_scope(scope);
    crate::search::digest::respected_files_where(root, 50_000, |p| {
        scope.is_empty() || crate::schema::model::scope::path_within(p, &scope)
    })
}
