//! Component `validate-growth`: the `growth` check (1.1.0).
//!
//! The early signal the 1-hour proof asked for: one file grew in every
//! session (218 -> 303 -> 354 -> 422 -> 506 lines) and harna said nothing
//! until `hotspot` noticed after the fifth. A file past half its size
//! budget that grew in most of its recent commits is named while splitting
//! it is still cheap. The history comes from `search::hotspots` (search
//! owns git); this turns it into findings.

use std::path::Path;

use super::{Severity, Violation};
use crate::schema::model::rules::Rules;

/// Recent commits of a file read, and how many of them must have grown it.
const RECENT: usize = 4;
const GREW_IN: usize = 3;
/// The default size budget a growing file is measured against.
const DEFAULT_BUDGET: usize = 600;
/// The least growth worth a note, in lines over the recent commits.
const MIN_ADDED: i64 = 60;

/// `srcs`: (repo-relative path, content) of the files this run read.
pub fn check(
    root: &Path,
    srcs: &[(String, String)],
    rules: &Rules,
    env: Option<&str>,
    out: &mut Vec<Violation>,
) {
    let budget_of = |file: &str| {
        super::budgets::bound_limit(rules, env, "growth", file)
            .or_else(|| super::budgets::bound_limit(rules, env, "component-granularity", file))
            .map(|l| l as usize)
            .unwrap_or(DEFAULT_BUDGET)
    };
    let candidates: Vec<(&str, usize, usize)> = srcs
        .iter()
        .filter(|(f, _)| f.rsplit('/').next() != Some("mod.rs"))
        .map(|(f, src)| (f.as_str(), src.lines().count(), budget_of(f)))
        .filter(|(_, lines, budget)| *lines >= budget / 2 && lines < budget)
        .collect();
    if candidates.is_empty() {
        return;
    }
    let paths: Vec<&str> = candidates.iter().map(|(f, _, _)| *f).collect();
    let history = crate::search::hotspots::recent_growth(root, 300, &paths);
    for (file, lines, budget) in candidates {
        let Some(deltas) = history.get(file) else {
            continue;
        };
        let recent: Vec<i64> = deltas.iter().take(RECENT).copied().collect();
        let grew = recent.iter().filter(|d| **d > 0).count();
        if recent.len() < GREW_IN || grew < GREW_IN {
            continue;
        }
        let added: i64 = recent.iter().filter(|d| **d > 0).sum();
        // material growth only: a file that gained at least MIN_ADDED lines
        // and a fifth of what it was; one born in the window is born-size's
        // to judge, not growth's (the first cut flagged 41 files on harna,
        // some for +5 lines)
        let before = lines as i64 - recent.iter().sum::<i64>();
        if before <= 0 || added < MIN_ADDED.max(before / 5) {
            continue;
        }
        out.push(Violation {
            check: "growth".into(),
            file: file.into(),
            line: 0,
            severity: Severity::Info,
            message: format!(
                "grew in {grew} of its last {} commits (+{added} lines), now {lines} of a {budget}-line budget: split it while that is still cheap",
                recent.len()
            ),
            hint: "`harna decompose <file>` plans the split; scaffold the new components, then move the code".into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("git for tests")
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A file that keeps growing past half its budget is named early; a
    /// file that stays put is not.
    #[test]
    fn a_file_that_keeps_growing_is_named_before_its_budget() {
        let root = std::env::temp_dir().join(format!("harna-growth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        let lines = |n: usize| {
            (0..n)
                .map(|i| format!("fn f{i}() {{}}\n"))
                .collect::<String>()
        };
        std::fs::write(root.join("calm.rs"), lines(400)).unwrap();
        for n in [80, 150, 220, 290, 360] {
            std::fs::write(root.join("big.rs"), lines(n)).unwrap();
            git(&root, &["add", "."]);
            git(
                &root,
                &[
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "user.name=t",
                    "commit",
                    "-qm",
                    "grow",
                ],
            );
        }
        let srcs = vec![
            ("big.rs".to_string(), lines(360)),
            ("calm.rs".to_string(), lines(400)),
        ];
        let mut out = Vec::new();
        check(&root, &srcs, &Rules::default(), None, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].file, "big.rs");
        assert!(out[0].message.contains("now 360 of a 600-line budget"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
