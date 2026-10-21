//! Checks group `approvals` (1.1.4): what the owner approved stays as it
//! was approved, and the changes the owner wants to be asked about land
//! with an approval.
//!
//! - `approval-drift`: an approval's files no longer hash to what was
//!   approved (an approved look, an approved contract changed since).
//! - `approval-required`: since the base, a change a `[[require]]` names
//!   landed without an approval: a new dependency (`dep:<name>`), or a
//!   change under a required path no current approval covers.
//! - `approvals-changed`: the register itself changed since the base —
//!   named for the reviewer, since an agent can edit a file as well as run
//!   a command (review of 1.1.4).
//!
//! Text and hashes only: harna renders nothing and runs nothing. Pixel
//! comparison stays with the project's own visual tests.

use std::path::Path;

use super::Violation;
use crate::schema::registers::approvals::{Approval, Approvals};
use crate::schema::store::Store;

pub mod deps;
pub mod required;

/// The hash's name, written before it: `fnv1a64:<16 hex>`. Fixed for good
/// (review of 1.1.4: Rust's `DefaultHasher` may change between releases,
/// and approvals are compared across machines and years).
const ALGO: &str = "fnv1a64";

fn fnv(bytes: &[u8], mut h: u64) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The repository's tracked files (cwd-relative to `root`); None without git.
fn tracked(root: &Path) -> Option<Vec<String>> {
    let out = std::process::Command::new("git")
        .stdin(std::process::Stdio::null())
        .args(["-c", "core.quotePath=false", "ls-files", "--cached"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.replace('\\', "/"))
            .collect(),
    )
}

/// The files an approval's paths match today, sorted: a file is itself
/// (even with `[id]` in its name), a folder is the tracked files under it,
/// anything else is a glob over the tracked files. Untracked files stay
/// out (review of 1.1.4: CI hashed a different set than the owner's tree).
pub fn files_of(root: &Path, paths: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    let mut listed: Option<Vec<String>> = None;
    let mut all = |root: &Path| -> Vec<String> {
        listed
            .get_or_insert_with(|| {
                tracked(root).unwrap_or_else(|| {
                    crate::search::digest::respected_files_where(
                        root,
                        crate::search::digest::MAX_FILES,
                        |_| true,
                    )
                })
            })
            .clone()
    };
    for p in paths {
        let p = p.replace('\\', "/");
        let p = p.trim_end_matches('/').to_string();
        if root.join(&p).is_file() {
            files.push(p);
        } else if root.join(&p).is_dir() {
            let dir = format!("{p}/");
            files.extend(all(root).into_iter().filter(|f| f.starts_with(&dir)));
        } else if p.contains(['*', '?']) {
            files.extend(
                all(root)
                    .into_iter()
                    .filter(|f| crate::schema::model::scope::glob_match(&p, f)),
            );
        }
    }
    files.sort();
    files.dedup();
    files
}

/// The content hash of a file set: each path with its body, line endings
/// not counted, so a rename changes it as an edit does.
pub fn hash(root: &Path, files: &[String]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in files {
        let body = std::fs::read(root.join(f)).unwrap_or_default();
        let body: Vec<u8> = body
            .iter()
            .enumerate()
            .filter(|(i, b)| !(**b == b'\r' && body.get(i + 1) == Some(&b'\n')))
            .map(|(_, b)| *b)
            .collect();
        h = fnv(f.as_bytes(), h);
        h = fnv(&[0], h);
        h = fnv(&body, h);
        h = fnv(b"\n", h);
    }
    format!("{ALGO}:{h:016x}")
}

/// 1.1.4's first hashes, written before the algorithm was named.
fn legacy_hash(root: &Path, files: &[String]) -> String {
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

/// How one approval stands today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// A dependency approval: nothing to hash.
    Dependency,
    Current,
    Changed,
    /// Its paths match no file any more.
    Gone,
}

/// The state of one approval, and the files its paths match.
pub fn state(root: &Path, a: &Approval) -> (State, Vec<String>) {
    if a.dependency().is_some() && a.paths.is_empty() {
        return (State::Dependency, Vec::new());
    }
    let files = files_of(root, &a.paths);
    if files.is_empty() {
        return (State::Gone, files);
    }
    let same = if a.hash.contains(':') {
        hash(root, &files) == a.hash
    } else {
        legacy_hash(root, &files) == a.hash
    };
    (if same { State::Current } else { State::Changed }, files)
}

/// Every approval's state, computed once per run (review of 1.1.4: drift,
/// coverage and the briefing each re-hashed every file).
pub fn states(root: &Path, reg: &Approvals) -> Vec<(State, Vec<String>)> {
    reg.approvals.iter().map(|a| state(root, a)).collect()
}

/// Is `file` frozen by an approval that still holds as approved?
pub fn covered(reg: &Approvals, states: &[(State, Vec<String>)], file: &str) -> bool {
    reg.approvals.iter().zip(states).any(|(a, (s, _))| {
        *s == State::Current
            && a.paths
                .iter()
                .any(|p| crate::schema::model::scope::glob_match(p, file) || p == file)
    })
}

/// All three checks. Nothing to do without an approvals register.
pub fn check(root: &Path, store: &Store, out: &mut Vec<Violation>) {
    let reg = match store.load_approvals() {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => {
            required::register_changed(root, &Approvals::default(), out);
            return;
        }
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
    let states = states(root, &reg);
    drift(&reg, &states, out);
    required::check(root, &reg, &states, out);
    required::register_changed(root, &reg, out);
}

fn drift(reg: &Approvals, states: &[(State, Vec<String>)], out: &mut Vec<Violation>) {
    for (a, (state, files)) in reg.approvals.iter().zip(states) {
        if matches!(state, State::Current | State::Dependency) {
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
            message: if *state == State::Gone {
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
