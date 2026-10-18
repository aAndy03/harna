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
                if s.pass { "PASS" } else { "FAIL" },
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
    let ledger = store.load_acknowledged().unwrap_or_default();
    let (live, acked) =
        crate::enforcement::validate::acknowledge::partition(&ledger, violations);
    let errors: Vec<_> = live
        .iter()
        .filter(|v| v.severity == crate::enforcement::validate::Severity::Error)
        .collect();
    let mut detail = format!(
        "{} violation(s): {} error, {} info, {} acknowledged",
        live.len() + acked.len(),
        errors.len(),
        live.len() - errors.len(),
        acked.len()
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
    let doctor = crate::composition::doctor::build(store, root)?;
    let fails: Vec<_> = doctor
        .checks
        .iter()
        .filter(|c| c.grade == crate::composition::doctor::Grade::Fail)
        .collect();
    let warns = doctor
        .checks
        .iter()
        .filter(|c| c.grade == crate::composition::doctor::Grade::Warn)
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
        Err(e) => (false, format!("parity ledger unreadable — cannot prove zero debts: {e:#}")),
        Ok(events) => {
            let open: usize = events.iter().map(|e| e.open_targets().len()).sum();
            if open == 0 {
                (true, "no open parity debts — every structural change accounted for".into())
            } else {
                (false, format!("{open} open parity target(s) — resolve with `harna parity resolve`"))
            }
        }
    };
    sections.push(GateSection { gate: "parity".into(), pass, detail });

    // 4) Map: a release without a current code map ships blind. An
    // empty manifest is a fresh repo, not a stale map — vacuously true.
    let manifest = store.load_manifest().unwrap_or_default();
    let (pass, detail) = if manifest.is_empty() {
        (true, "no manifest declared yet (fresh repo — nothing to map)".into())
    } else {
        match store.load_digest() {
            Ok(d) if d.extractor_version == crate::search::digest::EXTRACTOR_VERSION => (
                true,
                format!(
                    "digest current: {} modules, {} signatures",
                    d.module_count(),
                    d.signature_count()
                ),
            ),
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

    // 5) Tests: the boundary, stated, not smuggled. harna never
    // executes project code (non-goal `no-code-execution`), so no
    // release gate inside this tool can truthfully claim the suite
    // passed. The section passes structurally and tells the caller
    // what to do instead — a gate that cannot see a signal says so.
    sections.push(GateSection {
        gate: "tests".into(),
        pass: true,
        detail: "NOT RUN by preflight — run the suite yourself (`cargo test`), then release".into(),
    });

    let pass = sections.iter().all(|s| s.pass);
    Ok(PreflightReport { pass, sections })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_store(tag: &str) -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir()
            .join(format!("harna-preflight-{tag}-{}", std::process::id()));
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
        for gate in ["validation", "doctor", "parity", "map", "tests"] {
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

    #[test]
    fn the_tests_boundary_is_stated_not_smuggled() {
        let (dir, store) = fresh_store("boundary");
        let r = build(&store, &dir, 10_000).unwrap();
        let tests = r.sections.iter().find(|s| s.gate == "tests").unwrap();
        assert!(tests.detail.contains("NOT RUN by preflight"));
        assert!(tests.detail.contains("cargo test"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_parity_blocks_the_gate() {
        use crate::schema::digest_data::{Digest, Module};
        let (dir, store) = fresh_store("parity");
        // A fabricated open event through the real constructor: a removed
        // function whose importer was never reviewed (0.3.35: the only kind
        // of target that stays owed).
        let module = |path: &str, sigs: &[&str], deps: &[&str]| Module {
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
            &store, "test-event", "manual", "test", "HEAD", &baseline, &fresh, &d, &["x".to_string()],
        )
        .unwrap();
        assert!(!event.open_targets().is_empty(), "the fixture really is open");
        let r = build(&store, &dir, 10_000).unwrap();
        let parity = r.sections.iter().find(|s| s.gate == "parity").unwrap();
        assert!(!parity.pass);
        assert!(parity.detail.contains("open parity target(s)"), "{}", parity.detail);
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
