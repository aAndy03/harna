//! Check `approval-required` (1.1.4): a change the owner asked to be
//! consulted on landed without an approval. Read against the base
//! (`HEAD` locally, `HARNA_BASE` in CI), like the history checks.

use std::path::Path;

use super::super::Violation;
use crate::schema::model::scope::glob_match;
use crate::schema::registers::approvals::{Approvals, Require};

pub fn check(root: &Path, reg: &Approvals, out: &mut Vec<Violation>) {
    if reg.require.is_empty() {
        return;
    }
    let base = super::super::git::base_ref();
    let changed = super::super::git::changed_since(root, &base);
    if changed.is_empty() {
        return;
    }
    for r in &reg.require {
        match r.kind.as_str() {
            "dependency-added" => dependencies(root, reg, r, &base, &changed, out),
            "path" => paths(root, reg, r, &base, &changed, out),
            _ => {} // doctor names an unknown kind
        }
    }
}

fn severity() -> crate::enforcement::violation::Severity {
    crate::enforcement::checks::severity_of("approval-required")
}

fn why(r: &Require) -> String {
    if r.reason.is_empty() {
        String::new()
    } else {
        format!(" ({})", r.reason)
    }
}

fn dependencies(
    root: &Path,
    reg: &Approvals,
    r: &Require,
    base: &str,
    changed: &[(char, String)],
    out: &mut Vec<Violation>,
) {
    for (status, path) in changed {
        if *status == 'D'
            || !super::deps::is_manifest(path)
            || (!r.paths.is_empty() && !r.paths.iter().any(|g| glob_match(g, path)))
        {
            continue;
        }
        let Ok(now) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        let before = super::super::git::show_at(root, base, path)
            .map(|t| super::deps::names(path, &t))
            .unwrap_or_default();
        for name in super::deps::names(path, &now).difference(&before) {
            if reg.find(&format!("dep:{name}")).is_some() {
                continue;
            }
            out.push(Violation {
                check: "approval-required".into(),
                file: path.clone(),
                line: 0,
                severity: severity(),
                message: format!(
                    "new dependency `{name}` since {base} needs the owner's yes{}",
                    why(r)
                ),
                hint: format!(
                    "ask the owner first; once they agree, they run `harna approve dep:{name}`"
                ),
            });
        }
    }
}

fn paths(
    root: &Path,
    reg: &Approvals,
    r: &Require,
    base: &str,
    changed: &[(char, String)],
    out: &mut Vec<Violation>,
) {
    for (status, path) in changed {
        if !r.paths.iter().any(|g| glob_match(g, path)) {
            continue;
        }
        // a deleted file has nothing to hash: only a requirement-wide
        // approval of the folder can cover it
        if *status != 'D' && super::covered(root, reg, path) {
            continue;
        }
        let what = match status {
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            _ => "changed",
        };
        out.push(Violation {
            check: "approval-required".into(),
            file: path.clone(),
            line: 0,
            severity: severity(),
            message: format!(
                "{what} since {base} under {}, which needs the owner's yes{}: no approval covers it",
                r.paths.join(", "),
                why(r)
            ),
            hint: format!(
                "ask the owner first; once they agree, they run `harna approve <id> --paths {path}`"
            ),
        });
    }
}
