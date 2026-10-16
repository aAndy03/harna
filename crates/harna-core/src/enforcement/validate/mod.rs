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
//! - `mod.rs`     — orchestration + the advisory cram
//!                  signal (guidance, never a limit); the violation shape
//!                  itself lives in `enforcement/violation.rs` so registry
//!                  ↔ checker cannot form rings
//!
//! Variants of validation are added here, inside this container, and
//! registered in the manifest; importers consume this module's API.

use std::fs;
use std::path::Path;

use crate::schema::store::Store;

// The violation shape is re-exported: every check component continues to
// import `super::{Severity, Violation}` unchanged, while the registry
// (checks.rs) and other integrity users import `enforcement::violation`
// directly — no module can depend on both directions at once.
pub use crate::enforcement::violation::{Severity, Violation};

/// Advisory granularity signal (guidance, never a limit): a component file
/// that keeps accreting is a candidate to become its own container — exactly
/// how `validate.rs` became this `validate/` container. The threshold is a
/// trigger for reading, not a rule to be gamed.
const ACCRETION_LINES: usize = 600;

/// digest module path -> its home: the container whose path prefixes the
/// module (longest container path wins when containers nest) and that
/// container's layer. Lives in `search::digest` (0.3.12) so metrics shares
/// the one resolution; re-exported here so the check components keep
/// their `super::module_homes` paths unchanged.
pub(crate) use crate::search::digest::{module_homes, module_layers};

/// Run all checks over the whole repo.
pub fn run(root: &Path, store: &Store, max_files: usize) -> anyhow::Result<Vec<Violation>> {
    run_on(root, store, max_files, None, None)
}

/// Run checks over only the given `.rs` files (relative paths) — the
/// enforcement loop for pre-commit hooks and "validate after you edit".
/// `env` scopes the run: env-tagged rules for other environments stay out
/// of enforcement (bindings, severity overrides and pattern rules alike);
/// env-less rules always apply.
///
/// Manifest-level checks (broken component paths) always run: they are cheap
/// and read-only. File-level checks (URLs, layer boundaries, patterns) run on
/// the provided files only.
pub fn run_on(
    root: &Path,
    store: &Store,
    max_files: usize,
    files: Option<Vec<String>>,
    env: Option<&str>,
) -> anyhow::Result<Vec<Violation>> {
    let mut violations = Vec::new();
    let manifest = store.load_manifest().unwrap_or_default();

    // The registered-test obligation is a burst concept: it rides the
    // `--changed` enforcement loop only. Record the scope before `files`
    // is consumed below.
    let changed_scope = files.is_some();

    let rules_doc = store.load_rules().unwrap_or_default();
    let pattern_rules = pattern::compile(&rules_doc, env, &mut violations);

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
        let names = crate::search::digest::declared_names(&src);
        pattern::apply(&pattern_rules, file_rel, &names, &mut violations);
        srcs.push((file_rel.clone(), src));
    }

    if !manifest.is_empty() {
        let digest_ok = store.load_digest().ok();
        boundary::check(root, &manifest, &srcs, &mut violations);
        boundary::check_declared_surface(root, &manifest, digest_ok.as_ref(), &srcs, &mut violations);
        structure::check(root, &manifest, digest_ok.as_ref(), &mut violations);
        budgets::check(root, &manifest, &rules_doc, env, digest_ok.as_ref(), &mut violations);
        layering::check(root, &manifest, &rules_doc, env, digest_ok.as_ref(), &mut violations);
        transit::check(root, &manifest, &rules_doc, env, digest_ok.as_ref(), &mut violations);
        gateway::check(root, &manifest, digest_ok.as_ref(), &srcs, &mut violations);
        gateway::check_declarations(&manifest, &mut violations);
        manifest::check_paths_exist(root, &manifest, &mut violations);
        manifest::check_sequences(&manifest, &mut violations);
        manifest::check_layer_depends(&manifest, &mut violations);
        deny_sync::check(root, &manifest, &mut violations);
        test_coverage::check_declared(root, &manifest, &mut violations);
        if changed_scope {
            test_coverage::check_changes(root, &manifest, &rust_files, &mut violations);
        }

        // Reuse: duplication made visible. Body-level checks read the LIVE
        // sources of the run's file set; the single-owner half reads the
        // committed inventory.
        reuse::check_copy_paste(&srcs, &mut violations);
        reuse::check_single_owner(digest_ok.as_ref(), &mut violations);
        // Accretion with evidence: churn × items over the committed map
        // (0.3.14) — the line-count guidance above stays the cheap static
        // signal; this names which big files actually accrete pain.
        hotspot::check(root, digest_ok.as_ref(), &mut violations);
    }

    // Decision-log coherence: supersedes pointers stay real (no ghosts, no
    // cycles) so the chain is safe to render and follow.
    if store.decisions_dir().is_dir() {
        decisions::check(store, &mut violations);
    }

    check_parity_open(store, &mut violations);

    // Rule bindings come AFTER every check so the whole report is annotated
    // and severity-overridden — including decision-chain and parity-open
    // findings emitted below the old annotate point (before 0.3.1 they ran
    // unannotated). Unknown bindings warn; known ones annotate violations
    // with the rule's rationale; bound rules may override their check's
    // severity; the environment scopes every rule (registry: checks.rs).
    let bindings = rule_bindings::Bindings::from_rules(&rules_doc, env);
    bindings.check_unknown_bindings(&mut violations);
    bindings.check_pattern_coherence(&rules_doc, env, &mut violations);
    let (severity_overrides, mut binding_violations) =
        bindings.resolve_severity_overrides(&rules_doc, env);
    violations.append(&mut binding_violations);
    bindings.annotate(&mut violations);
    for v in violations.iter_mut() {
        if let Some(sev) = severity_overrides.get(v.check.as_str()) {
            v.severity = *sev;
        }
    }

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
pub mod budgets;
pub mod decisions;
pub mod deny_sync;
pub mod gateway;
pub mod git;
pub mod hotspot;
pub mod layering;
pub mod manifest;
pub mod pattern;
pub mod reuse;
pub mod rule_bindings;
pub mod structure;
pub mod test_coverage;
pub mod transit;
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
        let scoped = run_on(&root, &store, 100, Some(vec!["src/api/clean.rs".into()]), None).unwrap();
        assert!(scoped.is_empty());
        // Full scan finds the violation.
        let full = run(&root, &store, 100).unwrap();
        assert!(full.iter().any(|v| v.check == "no-hardcoded-urls"));
    }

    #[test]
    fn pattern_rules_run_env_scoped_through_run_on() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!("harna-pattern-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".harna")).unwrap();
        std::fs::write(
            root.join(".harna/rules.toml"),
            "schema_version = 1\n\n[[rules]]\nid = \"dev-naming\"\ntext = \"dev handlers keep the mockstripe adapter\"\nrationale = \"No real charges from dev sessions\"\norigin = \"test\"\nenv = \"dev\"\nseverity = \"error\"\npattern = \"(?i)mock_stripe\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src/api")).unwrap();
        std::fs::write(
            root.join("src/api/payment.rs"),
            "pub struct Mock_Stripe;\npub fn good() {}",
        )
        .unwrap();

        let store = crate::schema::store::Store::open(&root).unwrap();
        // No env declared: the dev rule is out of enforcement.
        let none = run_on(&root, &store, 100, None, None).unwrap();
        assert!(none.iter().all(|v| v.check != "pattern"), "env-less run ignores dev rules");
        // Declared prod: still out.
        let prod = run_on(&root, &store, 100, None, Some("prod")).unwrap();
        assert!(prod.iter().all(|v| v.check != "pattern"));
        // Declared dev: the pattern fires on the declared name, as an Error.
        let dev = run_on(&root, &store, 100, None, Some("dev")).unwrap();
        let hit = dev
            .iter()
            .find(|v| v.check == "pattern")
            .expect("dev run must see the pattern rule");
        assert_eq!(hit.severity, Severity::Error, "the rule's override wins");
        assert_eq!(hit.file, "src/api/payment.rs");
        assert!(hit.message.contains("Mock_Stripe"));
        assert!(hit.message.contains("declared name"), "names are policeable too");
        Ok(())
    }

    #[test]
    fn bad_pattern_regex_is_a_visible_violation_through_run_on() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!("harna-badpat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".harna")).unwrap();
        std::fs::write(
            root.join(".harna/rules.toml"),
            "schema_version = 1\n\n[[rules]]\nid = \"broken\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\npattern = \"(unclosed\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/x.rs"), "pub fn y() {}").unwrap();

        let store = crate::schema::store::Store::open(&root).unwrap();
        let v = run_on(&root, &store, 100, None, None).unwrap();
        assert!(v.iter().any(|viol| viol.message.contains("un-compilable pattern")));
        Ok(())
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
