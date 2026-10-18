//! Component `folders` (0.3.39): structure is folders, not file names.
//!
//! The line budgets keep files small; nothing kept *folders* small, so
//! agents kept adding files beside each other and encoded the grouping in
//! names instead (`acknowledged_data.rs`, `deferred_data.rs`, …). Two
//! checks push the grouping into the tree where it belongs:
//! - `folder-size` — a folder holds at most `limit` source files (the
//!   bound rule's limit; silent with no rule, like every budget). The
//!   folder's own `mod.rs` and its `tests.rs` sibling do not count.
//! - `name-cluster` — files in one folder that share a name token
//!   (`*_data.rs`, `cmd_*.rs`, or `x.rs` beside `x_*.rs`) at least
//!   `limit` times (default 3) are a folder that was never created.
//!   Advisory by default; a rule may promote it.

use std::collections::{BTreeMap, BTreeSet};

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::model::rules::Rules;
use crate::search::digest::Digest;

/// Files that belong to a folder without being one of its members.
fn counted(stem: &str) -> bool {
    !matches!(stem, "mod" | "tests" | "lib" | "main" | "build")
}

/// Source files per folder (repo-relative dir -> file stems).
fn folders(d: &Digest) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in &d.modules {
        let (dir, file) = m.path.rsplit_once('/').unwrap_or(("", m.path.as_str()));
        let Some(stem) = file.strip_suffix(".rs") else {
            continue;
        };
        if counted(stem) && !crate::search::digest::is_test_path(&m.path) {
            out.entry(dir.to_string())
                .or_default()
                .push(stem.to_string());
        }
    }
    out
}

fn limit_for(rules: &Rules, env: Option<&str>, check: &str, dir: &str) -> Option<u32> {
    rules
        .rules
        .iter()
        .filter(|r| !r.is_deprecated() && r.matches_env(env))
        .filter(|r| r.check.as_deref() == Some(check) && r.limit.is_some())
        .filter(|r| {
            r.scope.is_empty()
                || r.scope
                    .iter()
                    .any(|s| crate::schema::model::scope::path_within(dir, s))
        })
        .filter_map(|r| r.limit)
        .min()
}

fn severity(check: &str) -> Severity {
    checks::spec(check)
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Info)
}

pub fn check(rules: &Rules, env: Option<&str>, digest: Option<&Digest>, out: &mut Vec<Violation>) {
    let Some(d) = digest else { return };
    for (dir, stems) in folders(d) {
        if let Some(limit) = limit_for(rules, env, "folder-size", &dir) {
            if stems.len() > limit as usize {
                out.push(Violation {
                    check: "folder-size".into(),
                    file: dir.clone(),
                    line: 0,
                    severity: severity("folder-size"),
                    message: format!(
                        "folder holds {} source files (budget {limit}) — group the related ones into sub-folders",
                        stems.len()
                    ),
                    hint: "find the clusters (`harna decompose` on the folder's biggest file helps); \
                           `harna move` relocates files and keeps the manifest, rule scopes and mod tree linked"
                        .into(),
                });
            }
        }
        let min = limit_for(rules, env, "name-cluster", &dir).unwrap_or(3) as usize;
        for (token, members) in clusters(&stems) {
            let folder_name = dir.rsplit('/').next().unwrap_or(&dir);
            if members.len() < min || folder_name == token {
                continue;
            }
            out.push(Violation {
                check: "name-cluster".into(),
                file: dir.clone(),
                line: 0,
                severity: severity("name-cluster"),
                message: format!(
                    "{} files share the name token `{token}` ({}) — that is a folder that was never created",
                    members.len(),
                    members.iter().map(|m| format!("{m}.rs")).collect::<Vec<_>>().join(", ")
                ),
                hint: format!("move them under `{dir}/{token}/` with `harna move` (names can drop the token there)"),
            });
        }
    }
}

/// Name tokens shared by stems: the first and last `_` segments, and a
/// bare stem counts toward its own token (`parity.rs` + `parity_policy.rs`).
pub fn clusters(stems: &[String]) -> BTreeMap<String, BTreeSet<String>> {
    let mut by: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for s in stems {
        let parts: Vec<&str> = s.split('_').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            by.entry(s.clone()).or_default().insert(s.clone());
            continue;
        }
        by.entry(parts[0].to_string())
            .or_default()
            .insert(s.clone());
        by.entry(parts[parts.len() - 1].to_string())
            .or_default()
            .insert(s.clone());
    }
    // A token held only by the bare stem itself is not a cluster.
    by.retain(|token, members| !(members.len() == 1 && members.contains(token)));
    by
}

#[cfg(test)]
mod tests;
