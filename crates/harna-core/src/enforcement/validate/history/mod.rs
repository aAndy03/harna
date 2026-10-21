//! Checks group `history` (1.1.3): checks that read the change itself, not
//! only the tree it leaves. Both compare against `git::base_ref()` — the
//! working tree's change by default, a pull request's whole range when CI
//! sets `HARNA_BASE`.
//!
//! - `append-only`: files under the paths a rule declares (migrations, an
//!   event log's schema history) may be added, never edited or deleted once
//!   committed.
//! - `test-weakened`: a change that removes assertions or adds a skip is
//!   named, so the reason is recorded instead of assumed (agents "pass"
//!   suites by editing what they assert — ImpossibleBench, 2025).

use std::path::Path;

use super::Violation;
use crate::schema::model::rules::Rules;

pub mod append_only;
pub mod weakened;

/// Both history checks over the change since the base.
pub fn check(root: &Path, rules: &Rules, env: Option<&str>, out: &mut Vec<Violation>) {
    let base = super::git::base_ref();
    let changed = super::git::changed_since(root, &base);
    if changed.is_empty() {
        return;
    }
    append_only::check(rules, env, &base, &changed, out);
    weakened::check(root, &base, &changed, out);
}
