//! Checks group `artifacts` (0.3.48, codex/zed/ruff study R4, R5): the
//! files that belong with code. Crate conformity (companion files and
//! required Cargo keys per crate) and test data (snapshots no test
//! references any more).

use std::path::Path;

use super::Violation;
use crate::schema::model::rules::Rules;

pub mod conformity;
pub mod snapshots;

/// Every artifacts check, one call from validate.
pub fn check(
    root: &Path,
    rules: &Rules,
    env: Option<&str>,
    digest: Option<&crate::search::digest::Digest>,
    out: &mut Vec<Violation>,
) {
    conformity::check(root, rules, env, out);
    snapshots::check(root, rules, env, digest, out);
}
