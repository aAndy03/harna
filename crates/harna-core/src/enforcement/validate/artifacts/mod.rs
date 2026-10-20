//! Checks group `artifacts` (0.3.48, codex/zed/ruff study R4, R5): the
//! files that belong with code. Crate conformity (companion files and
//! required Cargo keys per crate) and test data (snapshots no test
//! references any more), and test reach (what each declared kind of test
//! may cross).

use std::path::Path;

use super::Violation;
use crate::schema::model::rules::Rules;

pub mod api_parity;
pub mod conformity;
pub mod contract;
pub mod deprecation;
pub mod kind_coverage;
pub mod snapshots;
pub mod test_reach;

/// Every artifacts check, one call from validate.
pub fn check(
    root: &Path,
    rules: &Rules,
    env: Option<&str>,
    digest: Option<&crate::search::digest::Digest>,
    manifest: &crate::schema::model::manifest::Manifest,
    with_snapshots: bool,
    out: &mut Vec<Violation>,
) {
    let mut t = super::timings::Timer::from_env();
    test_reach::check(root, manifest, out);
    t.mark("artifacts: test-reach");
    deprecation::check(root, rules, env, out);
    t.mark("artifacts: deprecation-window");
    kind_coverage::check(root, rules, env, manifest, out);
    t.mark("artifacts: kind-coverage");
    conformity::check(root, rules, env, out);
    t.mark("artifacts: crate-conformity");
    // every test file is read to find references: a scoped --changed run
    // skips it unless files were added or deleted (0.9.1)
    if with_snapshots {
        snapshots::check(root, rules, env, digest, out);
    }
    t.mark("artifacts: snapshots");
}
