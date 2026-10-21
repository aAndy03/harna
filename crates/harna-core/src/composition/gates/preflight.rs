//! Component `preflight` (0.3.29): the release gate as one command.
//!
//! Everything a release must survive, composed into a single PASS/FAIL —
//! validation errors, doctor failures, open parity debts, a stale or
//! missing code map. What preflight deliberately does NOT do is execute
//! project code: running the test suite is the caller's ritual step
//! (non-goal `no-code-execution`), so the report names the test suite
//! as NOT-RUN rather than pretending it passed. A gate that cannot see
//! a signal says so out loud.
//!
//! Exit contract (CLI): PASS prints the report and exits 0; FAIL prints
//! the same report and exits 2, the gate precedent `validate` set.

use std::path::Path;

use serde::Serialize;

use crate::schema::store::Store;

#[derive(Debug, Clone, Serialize)]
pub struct GateSection {
    /// Gate name: validation, doctor, parity, map, tests.
    pub gate: String,
    pub pass: bool,
    /// One line: what was checked and what it found.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreflightReport {
    pub pass: bool,
    pub sections: Vec<GateSection>,
}

impl PreflightReport {
    pub fn render(&self) -> String {
        let mut out = String::from("preflight (release gate — every line must hold):\n");
        for s in &self.sections {
            out.push_str(&format!(
                "  [{}] {}: {}\n",
                // the suite is named, not judged: no verdict to show
                if s.gate == "tests" {
                    "NOTE"
                } else if s.pass {
                    "PASS"
                } else {
                    "FAIL"
                },
                s.gate,
                s.detail
            ));
        }
        out.push_str(if self.pass {
            "preflight: PASS — releasable (the test suite itself ran outside this gate)\n"
        } else {
            "preflight: FAIL — fix the failing gates, then run the suite and preflight again\n"
        });
        out
    }
}

pub fn build(store: &Store, root: &Path, max_files: usize) -> anyhow::Result<PreflightReport> {
    let mut sections = Vec::new();

    // 1) Validation: error-severity findings block a release; Info
    // findings ride along as named context, never as blockers. The
    // advisory ledger applies here too (0.3.32): locked Infos are
    // receipts, not live findings — `validate` and `preflight` must
    // agree on what is locked.
    let violations = crate::enforcement::validate::run(root, store, max_files)?;
    // The one gate (0.3.44): validate and preflight cannot disagree on
    // what the ledgers absorb.
    let report = crate::enforcement::validate::acknowledge::gate_for(store, violations, false);
    let live = &report.violations;
    let errors: Vec<_> = live
        .iter()
        .filter(|v| v.severity == crate::enforcement::validate::Severity::Error)
        .collect();
    let mut detail = format!(
        "{} violation(s): {} error, {} info, {} acknowledged, {} baselined",
        live.len() + report.acknowledged.len(),
        errors.len(),
        live.len() - errors.len(),
        report.acknowledged.len(),
        report.baselined.iter().map(|b| b.found).sum::<usize>()
    );
    for e in errors.iter().take(3) {
        detail.push_str(&format!("\n      - [{}] {}", e.check, e.message));
    }
    if errors.len() > 3 {
        detail.push_str(&format!("\n      - … +{} more errors", errors.len() - 3));
    }
    sections.push(GateSection {
        gate: "validation".into(),
        pass: errors.is_empty(),
        detail,
    });

    // 2) Doctor: any failing installation/data check blocks — a release
    // from a broken harna home is unverifiable by definition.
    let doctor = crate::composition::gates::doctor::build(store, root)?;
    let fails: Vec<_> = doctor
        .checks
        .iter()
        .filter(|c| c.grade == crate::composition::gates::doctor::Grade::Fail)
        .collect();
    let warns = doctor
        .checks
        .iter()
        .filter(|c| c.grade == crate::composition::gates::doctor::Grade::Warn)
        .count();
    let mut detail = format!(
        "{} check(s): {} fail, {} warn",
        doctor.checks.len(),
        fails.len(),
        warns
    );
    for f in &fails {
        detail.push_str(&format!("\n      - [{}] {}", f.check, f.message));
    }
    sections.push(GateSection {
        gate: "doctor".into(),
        pass: fails.is_empty(),
        detail,
    });

    // 3) Parity: open structural debts mean the change this release
    // ships is not fully accounted for — the gate holds the release
    // until every target is acked, noop'd or verified.
    // An unreadable ledger fails the gate: it could be hiding open debts.
    let (pass, detail) = match crate::enforcement::parity::list_events(store) {
        Err(e) => (
            false,
            format!("parity ledger unreadable — cannot prove zero debts: {e:#}"),
        ),
        Ok(events) => {
            let open: usize = events.iter().map(|e| e.open_targets().len()).sum();
            if open == 0 {
                (
                    true,
                    "no open parity debts — every structural change accounted for".into(),
                )
            } else {
                (
                    false,
                    format!("{open} open parity target(s) — resolve with `harna parity resolve`"),
                )
            }
        }
    };
    sections.push(GateSection {
        gate: "parity".into(),
        pass,
        detail,
    });

    // 4) Map: a release without a current code map ships blind. An
    // empty manifest is a fresh repo, not a stale map — vacuously true.
    let manifest = store.load_manifest().unwrap_or_default();
    let (pass, detail) = if manifest.is_empty() {
        (
            true,
            "no manifest declared yet (fresh repo — nothing to map)".into(),
        )
    } else {
        match store.load_digest() {
            Ok(d) if d.extractor_version == crate::search::digest::EXTRACTOR_VERSION => {
                // the map against the files on disk (1.0.1, proof: a file
                // committed without a rescan passed as "digest current")
                let sources = crate::search::digest::respected_files_where(
                    root,
                    crate::search::digest::MAX_FILES,
                    |p| p.ends_with(".rs") || crate::search::digest::ts::is_source(p),
                );
                let mut paths: Vec<String> = sources;
                paths.extend(d.modules.iter().map(|m| m.path.clone()));
                paths.sort();
                paths.dedup();
                let stale = crate::search::digest::stale_paths(root, &d, &paths);
                if stale.is_empty() {
                    (
                        true,
                        format!(
                            "digest current: {} modules, {} signatures",
                            d.module_count(),
                            d.signature_count()
                        ),
                    )
                } else {
                    let shown: Vec<&str> = stale.iter().take(3).map(String::as_str).collect();
                    (
                        false,
                        format!(
                            "the map is stale for {} file(s) ({}{}) — run `harna scan` and commit",
                            stale.len(),
                            shown.join(", "),
                            if stale.len() > 3 { ", …" } else { "" }
                        ),
                    )
                }
            }
            Ok(_) => (
                false,
                "digest was written by an older extractor — run `harna scan`".into(),
            ),
            Err(_) => (
                false,
                "no readable digest — run `harna scan` before releasing".into(),
            ),
        }
    };
    sections.push(GateSection {
        gate: "map".into(),
        pass,
        detail,
    });

    // 5) Views (0.3.47): a committed view that no longer matches its data
    // ships a lie — what `harna render --check` gates, preflight gates
    // too. Only views the repo keeps are judged: a missing ARCHITECTURE.md
    // or SURFACE.md is a choice, a file without harna markers is prose.
    let views = crate::generation::views::render::committed_stale(
        root,
        &manifest,
        &store.load_effective_rules().unwrap_or_default(),
        &store.load_commands().unwrap_or_default(),
        store.load_digest().ok().as_ref(),
    )?;
    sections.push(GateSection {
        gate: "views".into(),
        pass: views.is_empty(),
        detail: if views.is_empty() {
            "committed views match their data".into()
        } else {
            format!(
                "stale: {} — run `harna render` and commit",
                views.join(", ")
            )
        },
    });

    // 5b) Ops readiness (0.8.3): every service answers every facet the
    // project requires before a release. Nothing required, nothing held.
    // an unreadable services.toml fails the gate (0.8.4: it read as "no
    // facets required" and passed)
    let ops = match store.load_services() {
        Err(e) => GateSection {
            gate: "ops".into(),
            pass: false,
            detail: format!("{e:#}"),
        },
        Ok(services) => {
            let unready = services.unready();
            // planned corridor messages owe their code before a release (1.0)
            let planned = services.planned_missing(&store.repo_root());
            GateSection {
                gate: "ops".into(),
                pass: unready.is_empty() && planned.is_empty(),
                // every reason at once (1.0 review: the planned list hid
                // the unanswered facets until it was fixed)
                detail: {
                    let mut owed = Vec::new();
                    if !planned.is_empty() {
                        owed.push(format!(
                            "planned corridor message(s) with no code yet: {} — write them, or drop them from the corridor",
                            planned
                                .iter()
                                .map(|(c, m)| format!("{c}/{m}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    if !unready.is_empty() {
                        owed.push(format!(
                            "unanswered: {} — harna service ready <service> <facet> \"<where, or n/a: why>\"",
                            unready
                                .iter()
                                .map(|(s, f)| format!("{s}/{f}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    if !owed.is_empty() {
                        owed.join("; ")
                    } else if services.require.is_empty() {
                        "no readiness facets required (harna service require health,backups,…)"
                            .into()
                    } else {
                        format!(
                            "{} service(s) answer {}",
                            services.services.len(),
                            services.require.join(", ")
                        )
                    }
                },
            }
        }
    };
    sections.push(ops);

    // 6) Tests: the boundary, stated, not smuggled. harna never
    // executes project code (non-goal `no-code-execution`), so no
    // release gate inside this tool can truthfully claim the suite
    // passed. The section passes structurally and tells the caller
    // what to do instead — a gate that cannot see a signal says so.
    // The project's own commands say how its suite runs (1.1.0: the note
    // said `cargo test` in a TypeScript repo too).
    let runs: Vec<String> = store
        .load_commands()
        .map(|c| {
            c.commands
                .iter()
                .filter(|x| x.name.contains("test") || x.proves.contains("test"))
                .map(|x| format!("`{}`", x.run))
                .collect()
        })
        .unwrap_or_default();
    sections.push(GateSection {
        gate: "tests".into(),
        pass: true,
        detail: if runs.is_empty() {
            "NOT RUN by preflight (harna runs no project code) — run the suite yourself; `harna command add test --run \"..\" --proves \"..\"` records how".into()
        } else {
            format!(
                "NOT RUN by preflight (harna runs no project code) — run {} yourself, then release",
                runs.join(", ")
            )
        },
    });

    let pass = sections.iter().all(|s| s.pass);
    Ok(PreflightReport { pass, sections })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_store(tag: &str) -> (std::path::PathBuf, Store) {
        let dir =
            std::env::temp_dir().join(format!("harna-preflight-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir).unwrap();
        store.init().unwrap();
        (dir, store)
    }

    #[test]
    fn a_fresh_repo_passes_warns_do_not_gate() {
        let (dir, store) = fresh_store("fresh");
        let r = build(&store, &dir, 10_000).unwrap();
        // No git repo here: doctor warns, and warns do not gate.
        assert!(r.pass, "fresh repo must clear preflight:\n{}", r.render());
        for gate in ["validation", "doctor", "parity", "map", "views", "tests"] {
            let s = r.sections.iter().find(|s| s.gate == gate).unwrap();
            assert!(s.pass, "{gate} should hold on a fresh repo: {}", s.detail);
        }
        // The advisory ledger composes here too: the counts name it even
        // when empty, so `validate` and `preflight` can never disagree on
        // what is locked.
        let validation = r.sections.iter().find(|s| s.gate == "validation").unwrap();
        assert!(
            validation.detail.contains("0 acknowledged"),
            "{}",
            validation.detail
        );
        let text = r.render();
        assert!(text.contains("preflight: PASS"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failing_doctor_check_blocks_the_gate_and_names_it() {
        let (dir, store) = fresh_store("corrupt");
        // Corrupt glossary: doctor fails the terms check (doctor's own
        // contract), so preflight must fail and name it.
        std::fs::write(store.terms_path(), "not = [valid toml {{{{").unwrap();
        let r = build(&store, &dir, 10_000).unwrap();
        assert!(!r.pass, "corrupt data file must block the release");
        let doctor = r.sections.iter().find(|s| s.gate == "doctor").unwrap();
        assert!(!doctor.pass);
        assert!(doctor.detail.contains("terms"), "{}", doctor.detail);
        let text = r.render();
        assert!(text.contains("[FAIL] doctor"));
        assert!(text.contains("preflight: FAIL"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 0.3.47: `render --check` and preflight cannot disagree — a harna
    /// block that drifted from its data blocks the release.
    #[test]
    fn a_stale_committed_view_blocks_the_gate() {
        let (dir, store) = fresh_store("views");
        std::fs::write(
            dir.join("AGENTS.md"),
            "# Prose\n<!-- harna:begin -->\nhand-edited\n<!-- harna:end -->\n",
        )
        .unwrap();
        let r = build(&store, &dir, 10_000).unwrap();
        let views = r.sections.iter().find(|s| s.gate == "views").unwrap();
        assert!(
            !views.pass && views.detail.contains("AGENTS.md"),
            "{}",
            views.detail
        );
        assert!(!r.pass);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tests_boundary_is_stated_not_smuggled() {
        let (dir, store) = fresh_store("boundary");
        let r = build(&store, &dir, 10_000).unwrap();
        let tests = r.sections.iter().find(|s| s.gate == "tests").unwrap();
        assert!(tests.detail.contains("NOT RUN by preflight"));
        // no command registered: how to record one (1.1.0)
        assert!(
            tests.detail.contains("harna command add test"),
            "{}",
            tests.detail
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_parity_blocks_the_gate() {
        use crate::schema::map::{Digest, Module};
        let (dir, store) = fresh_store("parity");
        // A fabricated open event through the real constructor: a removed
        // function whose importer was never reviewed (0.3.35: the only kind
        // of target that stays owed).
        let module = |path: &str, sigs: &[&str], deps: &[&str]| Module {
            reexports: Vec::new(),
            crate_dir: None,
            src_root: None,
            path: path.into(),
            lang: "rust".into(),
            signatures: sigs.iter().map(|s| s.to_string()).collect(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            body_hash: "h".into(),
            inventory: vec![],
            mods: vec![],
        };
        let baseline = Digest {
            modules: vec![
                module("crates/x/src/a.rs", &["pub fn f()"], &[]),
                module("crates/y/src/b.rs", &[], &["x::a::f"]),
            ],
            ..Default::default()
        };
        let fresh = Digest {
            modules: vec![
                module("crates/x/src/a.rs", &[], &[]),
                module("crates/y/src/b.rs", &[], &["x::a::f"]),
            ],
            ..Default::default()
        };
        let d = crate::search::diff::diff(&baseline, &fresh);
        let event = crate::enforcement::parity::open_from_digests(
            &store,
            "test-event",
            "manual",
            "test",
            "HEAD",
            &baseline,
            &fresh,
            &d,
            &["x".to_string()],
        )
        .unwrap();
        assert!(
            !event.open_targets().is_empty(),
            "the fixture really is open"
        );
        let r = build(&store, &dir, 10_000).unwrap();
        let parity = r.sections.iter().find(|s| s.gate == "parity").unwrap();
        assert!(!parity.pass);
        assert!(
            parity.detail.contains("open parity target(s)"),
            "{}",
            parity.detail
        );
        assert!(!r.pass);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreadable_parity_ledger_blocks_the_gate() {
        let (dir, store) = fresh_store("parity-broken");
        let ledger = dir.join(".harna/parity");
        std::fs::create_dir_all(&ledger).unwrap();
        std::fs::write(ledger.join("half.toml"), "id = \"half\"\ntargets = [{").unwrap();
        let r = build(&store, &dir, 10_000).unwrap();
        let parity = r.sections.iter().find(|s| s.gate == "parity").unwrap();
        assert!(!parity.pass, "an unreadable ledger cannot prove zero debts");
        assert!(parity.detail.contains("unreadable"), "{}", parity.detail);
        assert!(!r.pass);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
