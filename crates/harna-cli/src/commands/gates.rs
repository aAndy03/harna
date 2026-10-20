//! Gate arms: validate, doctor, preflight, validate-intent. Each prints its
//! report (prose or `--json`) and, when it blocks, exits non-zero so hooks
//! and CI gate on it — validate/preflight/intent with 2, doctor with 1.

use std::io::Write as _;
use std::path::Path;

use harna_core::schema::store::Store;

/// Explain on stderr (stdout stays the report), flush, exit with `code`.
fn block(code: i32, why: &str) -> ! {
    eprintln!("{why}");
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}

fn print_report<T: serde::Serialize>(
    json: bool,
    report: &T,
    prose: impl FnOnce() -> String,
) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
    } else {
        print!("{}", prose());
    }
    Ok(())
}

pub fn validate(
    store: &Store,
    root: &Path,
    changed: bool,
    max_files: usize,
    strict: bool,
    json: bool,
    env: Option<&str>,
) -> anyhow::Result<()> {
    // --changed reads the changed files live (0.9.1); a full run is told
    if !changed {
        if let Ok(d) = store.load_digest() {
            if let Some(n) = harna_core::composition::affected::map_note(root, &d) {
                eprintln!("warning: {n}");
            }
        }
    }
    let files = if changed {
        let changed_files = harna_core::enforcement::validate::changed_rust_files(root)?;
        // No changed .rs files still runs the manifest-level gates (parity
        // debts, manifest paths, decisions, roadmap, data health): a
        // manifest-only edit must not pass unchecked. Progress goes to
        // stderr so `--json` stays parseable.
        // TS/JS changes are checked too (0.9.2: the message said "no
        // changed .rs files" for a TS edit, which read as nothing checked)
        let sources = harna_core::enforcement::validate::git::changed_files(root)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.ends_with(".rs") || harna_core::search::digest::ts::is_source(f))
            .count();
        if sources == 0 {
            eprintln!("no changed source files — running manifest-level checks only");
        } else {
            eprintln!("checking {sources} changed source file(s)…");
        }
        Some(changed_files)
    } else {
        None
    };
    let violations = harna_core::enforcement::validate::run_on(root, store, max_files, files, env)?;
    // Both ledgers (0.3.31 advisory, 0.3.44 ratchet): locked Infos and
    // baselined errors ride as receipts, never as blocks.
    let report =
        harna_core::enforcement::validate::acknowledge::gate_for(store, violations, strict);
    let (errors, infos) = (report.errors, report.infos);
    let out = serde_json::json!({
        "violations": report.violations,
        "acknowledged": report.acknowledged,
        "baselined": report.baselined,
        "clean": report.violations.is_empty(),
        "counts": { "error": errors, "info": infos, "acknowledged": report.acknowledged.len(), "baselined": report.baselined.iter().map(|b| b.found).sum::<usize>() },
        "blocks": report.blocks,
    });
    print_report(json, &out, || report.render())?;
    if report.blocks {
        let mode = if strict && infos > 0 && errors == 0 {
            " — strict mode"
        } else {
            ""
        };
        block(
            2,
            &format!("harna: conformance gate failed ({errors} error, {infos} info{mode})"),
        );
    }
    Ok(())
}

pub fn doctor(store: &Store, root: &Path, json: bool) -> anyhow::Result<()> {
    let report = harna_core::composition::gates::doctor::build(store, root)?;
    print_report(json, &report, || report.render())?;
    if report.fail_count() > 0 {
        block(
            1,
            &format!(
                "harna: {} installation issue(s) block work — fix the FAIL items",
                report.fail_count()
            ),
        );
    }
    Ok(())
}

pub fn preflight(store: &Store, root: &Path, json: bool, max_files: usize) -> anyhow::Result<()> {
    let r = harna_core::composition::gates::preflight::build(store, root, max_files)?;
    print_report(json, &r, || r.render())?;
    if !r.pass {
        block(
            2,
            "harna: preflight FAIL — not releasable until every gate holds",
        );
    }
    Ok(())
}

pub fn intent(store: &Store, root: &Path, json: bool) -> anyhow::Result<()> {
    let r = harna_core::composition::gates::intent::build(store, root)?;
    print_report(json, &r, || r.render())?;
    if !r.pass {
        // Drift is normal mid-burst, but a claimed coherence with drift is
        // not releasable.
        block(
            2,
            "harna: validate-intent found DRIFT(s) — reconcile before claiming coherence",
        );
    }
    Ok(())
}
