//! Component `validate`: the conformance checker.
//!
//! Its concerns are components of their own (granularity principle):
//! - `urls`       — hardcoded-URL check
//! - `manifest`   — manifest coherence (paths, sequences, depends_on)
//! - `boundary`   — cross-layer imports on the real import graph
//! - `decisions`  — supersedes pointers in the decision log (no ghosts, no
//!                  cycles)
//! - `git`        — diff-scoped change selection
//! - `test_coverage` — registered tests earn their keep (declared paths exist;
//!   changed components had a test update in the burst)
//! - `mod.rs`     — orchestration + the violation type + the advisory cram
//!                  signal (guidance, never a limit)
//!
//! Variants of validation are added here, inside this container, and
//! registered in the manifest; importers consume this module's API.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::schema::store::Store;

/// One conformance violation found in the codebase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    /// Which check fired, e.g. "no-hardcoded-urls".
    pub check: String,
    /// File path relative to repo root.
    pub file: String,
    /// 1-based line number, or 0 when not line-specific.
    pub line: usize,
    /// Human/agent-readable description.
    pub message: String,
    /// Suggested fix, rendered for the agent.
    pub hint: String,
    /// Error violations gate commits/CI (exit code 2); info ones advise only.
    #[serde(default)]
    pub severity: Severity,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Error,
    Info,
}

impl Violation {
    /// Token-efficient rendering for agent consumption.
    pub fn render(&self) -> String {
        let marker = match self.severity {
            Severity::Error => "!",
            Severity::Info => "?",
        };
        format!(
            "[{marker}{}] {}:{} — {}\n  fix: {}",
            self.check,
            self.file,
            if self.line > 0 { self.line.to_string() } else { "-".into() },
            self.message,
            self.hint
        )
    }
}

/// Advisory granularity signal (guidance, never a limit): a component file
/// that keeps accreting is a candidate to become its own container — exactly
/// how `validate.rs` became this `validate/` container. The threshold is a
/// trigger for reading, not a rule to be gamed.
const ACCRETION_LINES: usize = 600;

/// Run all checks over the whole repo.
pub fn run(root: &Path, store: &Store, max_files: usize) -> anyhow::Result<Vec<Violation>> {
    run_on(root, store, max_files, None)
}

/// Run checks over only the given `.rs` files (relative paths) — the
/// enforcement loop for pre-commit hooks and "validate after you edit".
///
/// Manifest-level checks (broken component paths) always run: they are cheap
/// and read-only. File-level checks (URLs, layer boundaries) run on the
/// provided files only.
pub fn run_on(
    root: &Path,
    store: &Store,
    max_files: usize,
    files: Option<Vec<String>>,
) -> anyhow::Result<Vec<Violation>> {
    let mut violations = Vec::new();
    let manifest = store.load_manifest().unwrap_or_default();

    // The registered-test obligation is a burst concept: it rides the
    // `--changed` enforcement loop only. Record the scope before `files`
    // is consumed below.
    let changed_scope = files.is_some();

    let rust_files: Vec<String> = match files {
        Some(f) => f.into_iter().take(max_files).collect(),
        None => crate::search::digest::respected_rust_files(root, max_files),
    };

    let mut srcs: Vec<(String, String)> = Vec::new();
    for file_rel in &rust_files {
        let abs = root.join(file_rel);
        let src = match fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => continue,
        };
        urls::check(&src, file_rel, &mut violations);
        check_accretion(file_rel, &src, &mut violations);
        srcs.push((file_rel.clone(), src));
    }

    if !manifest.is_empty() {
        boundary::check(root, &manifest, &srcs, &mut violations);
        manifest::check_paths_exist(root, &manifest, &mut violations);
        manifest::check_sequences(&manifest, &mut violations);
        manifest::check_layer_depends(&manifest, &mut violations);
        deny_sync::check(root, &manifest, &mut violations);
        test_coverage::check_declared(root, &manifest, &mut violations);
        if changed_scope {
            test_coverage::check_changes(root, &manifest, &rust_files, &mut violations);
        }
    }

    // Rule bindings: unknown bindings warn; known bindings annotate their
    // violations so the agent gets the rule's rationale with the fix.
    let rules_doc = store.load_rules().unwrap_or_default();
    let bindings = rule_bindings::Bindings::from_rules(&rules_doc);
    bindings.check_unknown_bindings(&mut violations);
    bindings.annotate(&mut violations);

    // Decision-log coherence: supersedes pointers stay real (no ghosts, no
    // cycles) so the chain is safe to render and follow.
    if store.decisions_dir().is_dir() {
        decisions::check(store, &mut violations);
    }

    check_parity_open(store, &mut violations);

    // Deduplicate: a file can belong to several declared layer scopes.
    violations.dedup();

    violations.sort_by(|a, b| (&a.file, a.line, &a.check).cmp(&(&b.file, b.line, &b.check)));
    Ok(violations)
}

/// Advisory cram signal (Info): component accretion guidance.
fn check_accretion(file: &str, src: &str, out: &mut Vec<Violation>) {
    if file.rsplit('/').next() == Some("mod.rs") {
        return; // orchestrators are allowed to be lists
    }
    let lines = src.lines().count();
    if lines < ACCRETION_LINES {
        return;
    }
    out.push(Violation {
        check: "component-granularity".into(),
        file: file.into(),
        line: 0,
        severity: Severity::Info,
        message: format!(
            "component file has grown to {lines} lines — harna imposes no limit, \
             but if its concerns have diverged, consider promoting it to its own \
             container of components"
        ),
        hint: "split inside this engine's folder, register new components in the \
               manifest, and let importers keep consuming the engine API"
            .into(),
    });
}

/// Parity gate: unresolved parity debts block conformance. An open target
/// means a structural change propagated to a module that has not yet been
/// acknowledged (`ack` was done), recorded (`noop`, nothing needed), or
/// verified by harna (`auto`, surface unchanged).
fn check_parity_open(store: &Store, out: &mut Vec<Violation>) {
    let events = match crate::enforcement::parity::list_events(store) {
        Ok(e) => e,
        // No parity directory / unreadable ledger: skip silently (bootstrap).
        Err(_) => return,
    };
    for ev in events {
        let open = ev.open_targets().len();
        if open == 0 {
            continue;
        }
        out.push(Violation {
            check: "parity-open".into(),
            file: format!(".harna/parity/{}.toml", ev.id),
            line: 0,
            message: format!(
                "event `{}` (gen {}): {} unresolved parity target(s)",
                ev.id, ev.generation, open
            ),
            hint: "ack or noop each open target (harna_parity_resolve), or land the change in those modules".into(),
            severity: Severity::Error,
        });
    }
}

pub mod boundary;
pub mod decisions;
pub mod deny_sync;
pub mod git;
pub mod manifest;
pub mod rule_bindings;
pub mod test_coverage;
pub mod urls;

pub use git::changed_rust_files;
pub use manifest::check_layer_depends;
pub use manifest::check_sequences;
pub use urls::check as check_hardcoded_urls;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_on_scopes_file_checks_to_listed_files() {
        // A standalone file check through run_on's per-file path.
        let root = std::env::temp_dir().join(format!("harna-runon-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".harna")).unwrap();
        std::fs::write(root.join(".harna/rules.toml"), "schema_version = 1\n").unwrap();
        std::fs::create_dir_all(root.join("src/api")).unwrap();
        std::fs::write(
            root.join("src/api/handlers.rs"),
            "fn bad() { fetch(\"https://api.example.com\"); }",
        )
        .unwrap();
        std::fs::write(
            root.join("src/api/clean.rs"),
            "pub fn good() {}",
        )
        .unwrap();

        let store = crate::schema::store::Store::open(&root).unwrap();
        // Only the clean file is checked: no violations.
        let scoped = run_on(&root, &store, 100, Some(vec!["src/api/clean.rs".into()])).unwrap();
        assert!(scoped.is_empty());
        // Full scan finds the violation.
        let full = run(&root, &store, 100).unwrap();
        assert!(full.iter().any(|v| v.check == "no-hardcoded-urls"));
    }

    #[test]
    fn accretion_is_advisory_never_gating() {
        let big = "pub fn x() {}\n".repeat(700);
        let mut v = Vec::new();
        check_accretion("src/big.rs", &big, &mut v);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].severity, Severity::Info, "guidance, not a limit");
        assert!(v[0].message.contains("no limit"));
        assert_eq!(v[0].line, 0);
        // small file: nothing
        let mut v2 = Vec::new();
        check_accretion("src/small.rs", "pub fn x() {}", &mut v2);
        assert!(v2.is_empty());
    }
}
