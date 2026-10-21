//! Checks group `approvals` (1.1.4): what the owner approved stays as it
//! was approved, and the changes the owner wants to be asked about land
//! with an approval.
//!
//! - `approval-drift`: an approval's files no longer hash to what was
//!   approved (an approved look, an approved contract changed since).
//! - `approval-required`: since the base, a change a `[[require]]` names
//!   landed without an approval: a new dependency (`dep:<name>`), or a
//!   change under a required path no current approval covers.
//!
//! Text and hashes only: harna renders nothing and runs nothing. Pixel
//! comparison stays with the project's own visual tests.

use std::path::Path;

use super::Violation;
use crate::schema::registers::approvals::{Approval, Approvals};
use crate::schema::store::Store;

pub mod deps;
pub mod required;

/// The files an approval's paths match today, sorted. A plain path is
/// itself; a glob is matched over the repository's files.
pub fn files_of(root: &Path, paths: &[String]) -> Vec<String> {
    let globbed = paths.iter().any(|p| p.contains(['*', '?', '[']));
    let mut files: Vec<String> = paths
        .iter()
        .filter(|p| !p.contains(['*', '?', '[']))
        .map(|p| p.replace('\\', "/"))
        .filter(|p| root.join(p).is_file())
        .collect();
    if globbed {
        files.extend(crate::search::digest::respected_files_where(
            root,
            crate::search::digest::MAX_FILES,
            |f| {
                paths.iter().any(|g| {
                    g.contains(['*', '?', '[']) && crate::schema::model::scope::glob_match(g, f)
                })
            },
        ));
    }
    files.sort();
    files.dedup();
    files
}

/// The content hash of a file set: each path with its body's hash, so a
/// rename changes it as an edit does. Line endings do not count.
pub fn hash(root: &Path, files: &[String]) -> String {
    let mut all = String::new();
    for f in files {
        let body = std::fs::read(root.join(f)).unwrap_or_default();
        all.push_str(f);
        all.push('\0');
        all.push_str(&crate::search::digest::hash_body(&body));
        all.push('\n');
    }
    crate::search::digest::hash_body(all.as_bytes())
}

/// Is `file` frozen by an approval whose files still hash as approved?
pub fn covered(root: &Path, approvals: &Approvals, file: &str) -> bool {
    approvals.approvals.iter().any(|a| {
        !a.paths.is_empty()
            && a.paths
                .iter()
                .any(|p| crate::schema::model::scope::glob_match(p, file))
            && current(root, a)
    })
}

fn current(root: &Path, a: &Approval) -> bool {
    hash(root, &files_of(root, &a.paths)) == a.hash
}

/// Both checks. Nothing to do without an approvals register.
pub fn check(root: &Path, store: &Store, out: &mut Vec<Violation>) {
    let reg = match store.load_approvals() {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => return,
        Err(e) => {
            out.push(Violation {
                check: "harna-data".into(),
                file: ".harna/approvals.toml".into(),
                line: 0,
                severity: crate::enforcement::violation::Severity::Error,
                message: format!("approvals.toml unreadable: {e:#}"),
                hint: "fix the file (`harna doctor` names the problem)".into(),
            });
            return;
        }
    };
    drift(root, &reg, out);
    required::check(root, &reg, out);
}

fn drift(root: &Path, reg: &Approvals, out: &mut Vec<Violation>) {
    for a in reg.approvals.iter().filter(|a| !a.paths.is_empty()) {
        let files = files_of(root, &a.paths);
        if hash(root, &files) == a.hash {
            continue;
        }
        let when = match (a.by.is_empty(), a.date.is_empty()) {
            (false, false) => format!(" ({} on {})", a.by, a.date),
            (true, false) => format!(" ({})", a.date),
            _ => String::new(),
        };
        out.push(Violation {
            check: "approval-drift".into(),
            file: files
                .first()
                .cloned()
                .unwrap_or_else(|| ".harna/approvals.toml".into()),
            line: 0,
            severity: crate::enforcement::checks::severity_of("approval-drift"),
            message: if files.is_empty() {
                format!(
                    "`{}`'s approved files are gone ({}): what the owner approved{when} no longer exists",
                    a.label(),
                    a.paths.join(", ")
                )
            } else {
                format!(
                    "`{}` changed since the owner approved it{when}: {} file(s) differ from the approved look",
                    a.label(),
                    files.len()
                )
            },
            hint: format!(
                "show the owner the change, then the owner runs `harna approve {}` (the next version); or restore what was approved",
                a.id
            ),
        });
    }
}
