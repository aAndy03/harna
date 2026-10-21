//! Check `approval-required` (1.1.4): a change the owner asked to be
//! consulted on landed without an approval; and `approvals-changed`: the
//! register itself changed. Read against the base (`HEAD` locally,
//! `HARNA_BASE` in CI), like the history checks.

use std::collections::BTreeSet;
use std::path::Path;

use super::super::git::{self, Change};
use super::super::Violation;
use super::State;
use crate::schema::model::scope::glob_match;
use crate::schema::registers::approvals::{Approvals, Require};

pub fn check(
    root: &Path,
    reg: &Approvals,
    states: &[(State, Vec<String>)],
    out: &mut Vec<Violation>,
) {
    if reg.require.is_empty() {
        return;
    }
    let base = git::base_ref();
    // an unreadable base is named once, by the history checks
    let Ok(changed) = git::changed_since(root, &base) else {
        return;
    };
    if changed.is_empty() {
        return;
    }
    for r in &reg.require {
        match r.kind.as_str() {
            "dependency-added" => dependencies(root, reg, r, &base, &changed, out),
            "path" => paths(reg, states, r, &base, &changed, out),
            _ => {} // doctor names an unknown kind
        }
    }
}

fn severity(check: &str) -> crate::enforcement::violation::Severity {
    crate::enforcement::checks::severity_of(check)
}

fn why(r: &Require) -> String {
    if r.reason.is_empty() {
        String::new()
    } else {
        format!(" ({})", r.reason)
    }
}

/// Every dependency name the tracked manifests declare at `base`, or now.
/// The union, not file by file (review of 1.1.4: splitting a crate, or
/// moving a dependency into `[workspace.dependencies]`, is not a new one).
fn declared(root: &Path, base: Option<&str>) -> BTreeSet<String> {
    let args: Vec<&str> = match base {
        Some(b) => vec![
            "-c",
            "core.quotePath=false",
            "ls-tree",
            "-r",
            "--name-only",
            b,
        ],
        None => vec![
            "-c",
            "core.quotePath=false",
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    };
    let Ok(out) = std::process::Command::new("git")
        .stdin(std::process::Stdio::null())
        .args(&args)
        .current_dir(root)
        .output()
    else {
        return BTreeSet::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|p| super::deps::is_manifest(p) && !p.split('/').any(|s| s == "node_modules"))
        .flat_map(|p| {
            let text = match base {
                Some(b) => git::show_at(root, b, p),
                None => std::fs::read_to_string(root.join(p)).ok(),
            };
            text.map(|t| super::deps::names(p, &t)).unwrap_or_default()
        })
        .collect()
}

fn dependencies(
    root: &Path,
    reg: &Approvals,
    r: &Require,
    base: &str,
    changed: &[Change],
    out: &mut Vec<Violation>,
) {
    let manifests: Vec<&Change> = changed
        .iter()
        .filter(|c| {
            c.status != 'D'
                && super::deps::is_manifest(&c.path)
                && (r.paths.is_empty() || r.paths.iter().any(|g| glob_match(g, &c.path)))
        })
        .collect();
    if manifests.is_empty() {
        return;
    }
    let before = declared(root, Some(base));
    let now = declared(root, None);
    for name in now.difference(&before) {
        if reg.find(&format!("dep:{name}")).is_some() {
            continue;
        }
        // named at the changed manifest that declares it
        let Some(at) = manifests.iter().find(|c| {
            std::fs::read_to_string(root.join(&c.path))
                .map(|t| super::deps::names(&c.path, &t).contains(name))
                .unwrap_or(false)
        }) else {
            continue;
        };
        out.push(Violation {
            check: "approval-required".into(),
            file: at.path.clone(),
            line: 0,
            severity: severity("approval-required"),
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

fn paths(
    reg: &Approvals,
    states: &[(State, Vec<String>)],
    r: &Require,
    base: &str,
    changed: &[Change],
    out: &mut Vec<Violation>,
) {
    for c in changed {
        // a rename touches both its old and its new path
        let touched: Vec<&String> = std::iter::once(&c.path).chain(c.from.as_ref()).collect();
        for path in touched {
            if !r.paths.iter().any(|g| glob_match(g, path)) {
                continue;
            }
            // a deletion is covered by a current approval over its folder
            if super::covered(reg, states, path) {
                continue;
            }
            let what = match (c.status, c.from.as_ref() == Some(path)) {
                ('R', true) => "renamed away",
                ('R', false) => "renamed in",
                ('A', _) => "added",
                ('D', _) => "deleted",
                _ => "changed",
            };
            out.push(Violation {
                check: "approval-required".into(),
                file: path.clone(),
                line: 0,
                severity: severity("approval-required"),
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
}

/// `approvals-changed`: what the register gained or lost since the base,
/// for the reviewer to confirm the owner made it. An agent can edit the
/// file as well as run a command; this makes such an edit visible.
pub fn register_changed(root: &Path, reg: &Approvals, out: &mut Vec<Violation>) {
    const FILE: &str = ".harna/approvals.toml";
    let base = git::base_ref();
    let before: Approvals = git::show_at(root, &base, FILE)
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default();
    if &before == reg {
        return;
    }
    let mut what = Vec::new();
    for a in &reg.approvals {
        match before.find(&a.id) {
            None => what.push(format!("approved {}", a.label())),
            Some(b) if b.version != a.version || b.hash != a.hash || b.paths != a.paths => {
                what.push(format!("re-approved {}", a.label()))
            }
            _ => {}
        }
    }
    for b in &before.approvals {
        if reg.find(&b.id).is_none() {
            what.push(format!("withdrew {}", b.label()));
        }
    }
    for r in &before.require {
        if !reg.require.contains(r) {
            what.push(format!("dropped the requirement \"{}\"", r.line()));
        }
    }
    for r in &reg.require {
        if !before.require.contains(r) {
            what.push(format!("required \"{}\"", r.line()));
        }
    }
    if what.is_empty() {
        return;
    }
    out.push(Violation {
        check: "approvals-changed".into(),
        file: FILE.into(),
        line: 0,
        severity: severity("approvals-changed"),
        message: format!("the owner's register changed since {base}: {}", what.join("; ")),
        hint: "confirm the owner made these (`harna approve` is theirs alone); an agent's edit is reverted".into(),
    });
}
