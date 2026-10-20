//! Component `validate`: the conformance checker.
//!
//! Its concerns are components of their own (granularity principle):
//! - `urls`       — hardcoded-URL check
//! - `manifest`   — manifest coherence (paths, sequences, depends_on)
//! - `boundary`   — cross-layer imports on the real import graph
//! - `decisions`  — supersedes pointers in the decision log (no ghosts, no
//!   cycles)
//! - `git`        — diff-scoped change selection
//! - `test_coverage` — registered tests earn their keep (declared paths exist;
//!   changed components had a test update in the burst)
//! - `mod.rs`     — orchestration + the advisory cram
//!   signal (guidance, never a limit); the violation shape
//!   itself lives in `enforcement/violation.rs` so registry
//!   ↔ checker cannot form rings
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
    let mut timer = timings::Timer::from_env();
    let mut violations = Vec::new();
    // Unreadable data fails closed: a file that exists but will not load
    // becomes an Error emitted after severity overrides (so no binding can
    // downgrade it), instead of an empty default that skips every check.
    let mut data_errors = Vec::new();
    let manifest = load_or_flag(
        store.manifest_path().exists(),
        store.load_manifest(),
        ".harna/manifest.toml",
        &mut data_errors,
    );

    // The registered-test obligation is a burst concept: it rides the
    // `--changed` enforcement loop only. Record the scope before `files`
    // is consumed below.
    let changed_scope = files.is_some();

    let mut rules_doc = load_or_flag(
        store.rules_path().exists(),
        store.load_effective_rules(),
        ".harna/rules.toml",
        &mut data_errors,
    );
    timer.mark("load manifest + rules");

    let rust_files: Vec<String> = match files {
        Some(f) => f.into_iter().take(max_files).collect(),
        None => crate::search::digest::respected_rust_files(root, max_files),
    };

    // The map-backed checks (and the body-hash reuse below) share one map
    // read (0.3.17 one-scan); manifest-less repos never need it.
    let meaning_scoped = rules_doc.rules.iter().any(|r| r.scope_signature.is_some());
    let digest_ok = if !manifest.is_empty() || meaning_scoped {
        store.load_digest().ok()
    } else {
        None
    };
    // Rules scoped by meaning become concrete file scopes (0.4.8, R6), so
    // every check below honors them without knowing.
    // Before any check compiles its rules (0.4.9: pattern rules used to
    // compile first and ran meaning-scoped ones everywhere). Files the map
    // has not seen, and this burst's changed files, are matched live.
    if meaning_scoped {
        let known: std::collections::HashSet<&str> = digest_ok
            .as_ref()
            .map(|d| d.modules.iter().map(|m| m.path.as_str()).collect())
            .unwrap_or_default();
        let live: Vec<(String, String)> = rust_files
            .iter()
            .filter(|f| changed_scope || !known.contains(f.as_str()))
            .filter_map(|f| {
                std::fs::read_to_string(root.join(f))
                    .ok()
                    .map(|s| (f.clone(), s))
            })
            .collect();
        let found = meaning::expand(&mut rules_doc, digest_ok.as_ref(), &live);
        for (id, why) in found.broken {
            data_errors.push(Violation {
                check: "harna-data".into(),
                file: ".harna/rules.toml".into(),
                line: 0,
                message: format!("rule `{id}`: {why} — it applies nowhere until fixed"),
                hint: "fix the rule's scope_signature (a regex over declarations) or its check"
                    .into(),
                severity: Severity::Error,
            });
        }
        for (id, why) in found.degraded {
            violations.push(Violation {
                check: "rules-coverage".into(),
                file: ".harna/rules.toml".into(),
                line: 0,
                message: format!("rule `{id}`: {why}"),
                hint: "run `harna scan` so the map knows what the code declares".into(),
                severity: Severity::Info,
            });
        }
    }
    let pattern_rules = pattern::compile(&rules_doc, env, &mut violations);
    let code_rules = content::pattern::code::compile(&rules_doc, env, &mut data_errors);
    // plugin API surfaces (0.5.2) for api-parity; a broken pack is already
    // a data error through the effective rules
    // one broken pack is named, the others still apply (0.5.9)
    let (plugins, broken_plugins) = store.load_plugins_lenient();
    for why in broken_plugins {
        data_errors.push(Violation {
            check: "harna-data".into(),
            file: ".harna/plugins/".into(),
            line: 0,
            message: format!("a plugin does not load, so its rules do not apply: {why}"),
            hint: "fix it under .harna/plugins/, re-add it (harna plugin add), or remove it".into(),
            severity: Severity::Error,
        });
    }
    let plugin_apis: Vec<crate::schema::store::plugins::Api> =
        plugins.iter().filter_map(|p| p.api.clone()).collect();
    let surfaces = artifacts::api_parity::prepare(&rules_doc, env, &plugin_apis);

    // Born files (0.3.26): the working tree's BORN `.rs` files (added /
    // untracked per `git::new_rust_files`) feed the `born-size` birth
    // budget. A non-git repo contributes an empty list — the born-size
    // check then stays silent, consistent with its git-native scoping.
    let generated = hygiene::generated::outputs(&rules_doc, env);
    // Files no crate target reaches are data, not source (0.3.47, H10):
    // named once below, then skipped by every check that reads source.
    let orphans: std::collections::HashSet<&str> = digest_ok
        .as_ref()
        .map(|d| d.orphans.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let born_files: Vec<String> = if !manifest.is_empty() {
        git::new_rust_files(root)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| !hygiene::generated::is_generated(&generated, f))
            .filter(|f| !orphans.contains(f.as_str()))
            .collect()
    } else {
        Vec::new()
    };

    // Body-hash reuse (0.3.17): the map catalogs each module's declared
    // names. When the map is current-extractor AND a file's live body hash
    // equals the recorded one, that catalog is provably the parse — the
    // pattern rules consume it without re-parsing the file.
    let map_modules: std::collections::HashMap<&str, &crate::search::digest::Module> =
        match digest_ok
            .as_ref()
            .filter(|d| d.extractor_version == crate::search::digest::EXTRACTOR_VERSION)
        {
            Some(d) => d.modules.iter().map(|m| (m.path.as_str(), m)).collect(),
            None => std::collections::HashMap::new(),
        };

    let mut srcs: Vec<(String, String)> = Vec::new();
    for file_rel in &rust_files {
        let abs = root.join(file_rel);
        let src = match fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => continue,
        };
        // A whole test file gets the exemptions an inline test section has:
        // fixtures are not shipped code (0.3.37 — tests moved out of line
        // must not start failing boundary/gateway/URL checks).
        if crate::search::digest::is_test_path(file_rel) {
            continue;
        }
        if orphans.contains(file_rel.as_str()) {
            violations.push(unreachable(file_rel));
            continue;
        }
        urls::check(&src, file_rel, &mut violations);
        panics::check(&rules_doc, env, file_rel, &src, &mut violations);
        content::banned::check(&rules_doc, env, file_rel, &src, &mut violations);
        content::pattern::code::check(&code_rules, file_rel, &src, &mut violations);
        artifacts::api_parity::check(&surfaces, file_rel, &src, &mut violations);
        content::suppressions::check(file_rel, &src, &mut violations);
        // Generated outputs carry no size budget (0.3.46).
        let accretion = if hygiene::generated::is_generated(&generated, file_rel) {
            usize::MAX
        } else {
            budgets::bound_limit(&rules_doc, env, "component-granularity", file_rel)
                .map(|l| l as usize)
                .unwrap_or(ACCRETION_LINES)
        };
        check_accretion(file_rel, &src, accretion, &mut violations);
        let names = map_modules
            .get(file_rel.as_str())
            .and_then(|m| crate::search::digest::reusable_declared_names(m, &src))
            .unwrap_or_else(|| crate::search::digest::declared_names(&src));
        pattern::apply(&pattern_rules, file_rel, &names, &mut violations);
        srcs.push((file_rel.clone(), src));
    }
    timer.mark("per-file checks (urls, panics, banned, suppressions, accretion, patterns)");

    content::sync::check(root, &rules_doc, env, &mut violations);
    timer.mark("sync-group");
    content::banned::check_drift(root, &rules_doc, &mut violations);
    timer.mark("clippy drift");
    hygiene::check(root, &rules_doc, env, digest_ok.as_ref(), &mut violations);
    timer.mark("hygiene");
    // Project-wide registries (0.5.3) need every file: a burst validate
    // (--changed) would miss the uses outside it, so only the full run judges.
    if !changed_scope {
        content::pattern::registry::check(&rules_doc, env, &srcs, &mut violations);
    }
    artifacts::check(
        root,
        &rules_doc,
        env,
        digest_ok.as_ref(),
        &manifest,
        &mut violations,
    );
    timer.mark("artifacts (crate-conformity, unreferenced-snapshot)");

    if !manifest.is_empty() {
        boundary::check(root, &manifest, &srcs, &mut violations);
        // frozen contracts against today's surface (0.7.2)
        artifacts::contract::check(store, &manifest, digest_ok.as_ref(), &mut violations);
        // TypeScript/JavaScript containers (0.6.2): the same words over file edges
        if let Some(d) = digest_ok.as_ref() {
            boundary::ts::check(root, &manifest, d, &mut violations);
        }
        timer.mark("layer-boundary");
        boundary::check_declared_surface(
            root,
            &manifest,
            digest_ok.as_ref(),
            &srcs,
            &mut violations,
        );
        structure::check(root, &manifest, digest_ok.as_ref(), &mut violations);
        timer.mark("declared-surface + no-cycles");
        coherence::cargo_layering::check(root, &manifest, &mut violations);
        timer.mark("cargo-layering");
        budgets::check(
            root,
            &manifest,
            &rules_doc,
            env,
            digest_ok.as_ref(),
            &born_files,
            &mut violations,
        );
        timer.mark("budgets");
        layering::check(
            root,
            &manifest,
            &rules_doc,
            env,
            digest_ok.as_ref(),
            &mut violations,
        );
        transit::check(
            root,
            &manifest,
            &rules_doc,
            env,
            digest_ok.as_ref(),
            &mut violations,
        );
        gateway::check(root, &manifest, digest_ok.as_ref(), &srcs, &mut violations);
        gateway::check_declarations(&manifest, &mut violations);
        timer.mark("layering + transit + gateways");
        // plugin capabilities (0.5.4) expand into the tokens they stand for
        let capabilities: std::collections::BTreeMap<String, Vec<String>> = plugins
            .iter()
            .flat_map(|p| {
                p.meta
                    .capabilities
                    .iter()
                    .map(move |(k, v)| (format!("{}:{k}", p.meta.name), v.clone()))
            })
            .collect();
        let (owned, unknown) = boundaries::ownership::expand(&manifest, &capabilities);
        for (container, cap) in unknown {
            data_errors.push(Violation {
                check: "harna-data".into(),
                file: ".harna/manifest.toml".into(),
                line: 0,
                message: format!(
                    "container `{container}` owns `{cap}`, a capability no installed plugin declares — it guards nothing"
                ),
                hint: "install the plugin (harna plugin add), fix the name (harna plugin show <name> lists capabilities), or drop it".into(),
                severity: Severity::Error,
            });
        }
        boundaries::ownership::check(&owned, &srcs, &mut violations);
        timer.mark("ownership");
        roles::check(
            root,
            &manifest,
            &rules_doc,
            env,
            digest_ok.as_ref(),
            &mut violations,
        );
        manifest::check_paths_exist(root, &manifest, &mut violations);
        manifest::check_sequences(&manifest, &mut violations);
        manifest::check_layer_depends(&manifest, &mut violations);
        timer.mark("roles + manifest coherence");
        deny_sync::check(root, &manifest, &mut violations);
        timer.mark("deny-sync");
        test_coverage::check_declared(root, &manifest, &mut violations);
        timer.mark("tests-declared");
        if changed_scope {
            test_coverage::check_changes(root, &manifest, &rust_files, &mut violations);
        }

        // Reuse: duplication made visible. Body-level checks read the LIVE
        // sources of the run's file set; the single-owner half reads the
        // committed inventory.
        reuse::check_copy_paste(&srcs, &mut violations);
        timer.mark("no-copy-paste");
        reuse::check_single_owner(digest_ok.as_ref(), &mut violations);
        timer.mark("single-owner");
        // Rename linkage (0.3.31): declared paths that moved keep their
        // linkage — candidates by file name, never a second nag for the
        // plain-missing case manifest-path already owns.
        rename::check(root, &manifest, &mut violations);
        // Accretion with evidence: churn × items over the committed map
        // (0.3.14) — the line-count guidance above stays the cheap static
        // signal; this names which big files actually accrete pain.
        hotspot::check(root, digest_ok.as_ref(), &rules_doc, env, &mut violations);
        timer.mark("rename + hotspot");
    }

    // Folder budgets need only the map, not a manifest: a repo hears about
    // the folder it never created before it adopts anything (0.3.39).
    let any_map = if digest_ok.is_some() {
        None
    } else {
        store.load_digest().ok()
    };
    folders::check(
        &rules_doc,
        env,
        digest_ok.as_ref().or(any_map.as_ref()),
        &mut violations,
    );

    // Decision-log coherence: supersedes pointers stay real (no ghosts, no
    // cycles) so the chain is safe to render and follow.
    if store.decisions_dir().is_dir() {
        decisions::check(store, &mut violations);
    }

    // Roadmap coherence (0.3.28): the declared milestones stay sound data,
    // and deferred reviews keyed to milestones point at something declared.
    if store.roadmap_path().exists() {
        roadmap::check(store, &mut violations);
    }

    check_parity_open(store, &mut violations, &mut data_errors);
    crate::enforcement::parity::check_shape(store, &mut violations);

    // Rule bindings come AFTER every check so the whole report is annotated
    // and severity-overridden — including decision-chain and parity-open
    // findings emitted below the old annotate point (before 0.3.1 they ran
    // unannotated). Unknown bindings warn; known ones annotate violations
    // with the rule's rationale; bound rules may override their check's
    // severity; the environment scopes every rule (registry: checks.rs).
    let bindings = rule_bindings::Bindings::from_rules(&rules_doc, env);
    bindings.check_unknown_bindings(&mut violations);
    bindings.check_pattern_coherence(&rules_doc, env, &mut violations);
    bindings.check_limit_coherence(&rules_doc, env, &mut violations);
    let (severity_overrides, mut binding_violations) =
        bindings.resolve_severity_overrides(&rules_doc, env);
    violations.append(&mut binding_violations);
    bindings.annotate(&mut violations);
    for v in violations.iter_mut() {
        if let Some(sev) = severity_overrides.get(v.check.as_str()) {
            v.severity = *sev;
        }
    }
    // Unadopted content checks advise (0.3.44): nothing gates a repo on a
    // rule nobody stated. Memoized per check id — the report can be long.
    let mut adopted: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    for v in violations.iter_mut() {
        let Some(spec) = crate::enforcement::checks::spec(&v.check) else {
            continue;
        };
        if !spec.adopted_by_binding || v.severity != Severity::Error {
            continue;
        }
        let bound = *adopted
            .entry(v.check.clone())
            .or_insert_with(|| bindings.binds(&rules_doc, env, &v.check));
        if !bound {
            v.severity = Severity::Info;
            v.hint.push_str(&format!(
                " (advisory until a rule binds it: harna add-rule --id {0} --check {0} …)",
                v.check
            ));
        }
    }

    violations.append(&mut data_errors);
    timer.mark("decisions, roadmap, bindings");
    eprint!("{}", timer.report());

    // Sort, then deduplicate: a file can belong to several declared layer
    // scopes, and `dedup` only drops adjacent duplicates.
    violations.sort_by(|a, b| (&a.file, a.line, &a.check).cmp(&(&b.file, b.line, &b.check)));
    violations.dedup();
    Ok(violations)
}

/// A `.harna/` document that is absent reads as empty (a repo not yet
/// initialized); one that is present but unreadable is a `harna-data`
/// Error — never a silent default.
fn load_or_flag<T: Default>(
    present: bool,
    loaded: anyhow::Result<T>,
    file: &str,
    out: &mut Vec<Violation>,
) -> T {
    match loaded {
        Ok(v) => v,
        Err(e) => {
            if present {
                out.push(data_error(file, &e));
            }
            T::default()
        }
    }
}

fn data_error(file: &str, e: &anyhow::Error) -> Violation {
    Violation {
        check: "harna-data".into(),
        file: file.into(),
        line: 0,
        message: format!("unreadable harna data — checks that depend on it cannot run: {e:#}"),
        hint: "fix the file (or `harna restore <name>` from .harna/backup/); validation stays red until it loads".into(),
        severity: Severity::Error,
    }
}

/// Advisory (Info): a `.rs` file the module tree never reaches.
fn unreachable(file: &str) -> Violation {
    Violation {
        check: "unreachable-module".into(),
        file: file.into(),
        line: 0,
        severity: Severity::Info,
        message: "no crate target's module tree reaches this file — read as data                   (fixture), not source; source checks skip it"
            .into(),
        hint: "if it is meant to compile, declare it with `mod` from its parent;                otherwise nothing to do"
            .into(),
    }
}

/// Advisory cram signal (Info): component accretion guidance.
fn check_accretion(file: &str, src: &str, limit: usize, out: &mut Vec<Violation>) {
    if file.rsplit('/').next() == Some("mod.rs") {
        return; // orchestrators are allowed to be lists
    }
    let lines = src.lines().count();
    if lines < limit {
        return;
    }
    out.push(Violation {
        check: "component-granularity".into(),
        file: file.into(),
        line: 0,
        severity: Severity::Info,
        message: format!(
            "component file has grown to {lines} lines (advisory past {limit}{}; \
             harna imposes no limit, but if its concerns have diverged, \
             consider promoting it to its own container of components",
            if limit == ACCRETION_LINES {
                ""
            } else {
                ", rules.toml"
            }
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
fn check_parity_open(store: &Store, out: &mut Vec<Violation>, data_errors: &mut Vec<Violation>) {
    let events = match crate::enforcement::parity::list_events(store) {
        Ok(e) => e,
        // No parity directory reads as Ok(empty) (bootstrap). An event file
        // that will not parse could hide every open debt: fail closed.
        Err(e) => {
            data_errors.push(data_error(".harna/parity/", &e));
            return;
        }
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

pub mod acknowledge;
pub mod artifacts;
pub mod boundaries;
pub mod coherence;
pub mod content;
pub mod git;
pub mod hygiene;
pub mod meaning;
pub mod ratchet;
pub mod rule_bindings;
pub mod sizing;
pub mod timings;

// The checks, by group; `run_on` calls them by name.
use boundaries::{boundary, deny_sync, gateway, layering, roles, transit};
use coherence::{decisions, manifest, rename, roadmap, structure, test_coverage};
use content::{panics, pattern, reuse, urls};
use sizing::{budgets, folders, hotspot};

pub use coherence::manifest::check_layer_depends;
pub use coherence::manifest::check_sequences;
pub use content::urls::check as check_hardcoded_urls;
pub use git::changed_rust_files;

#[cfg(test)]
mod tests;
