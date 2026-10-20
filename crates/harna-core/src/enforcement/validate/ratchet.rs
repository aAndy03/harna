//! Component `ratchet` (0.3.44): the baseline ledger's enforcement half —
//! pure logic over findings, no filesystem.
//!
//! - `partition`: errors within their (check, file) baseline become
//!   receipts; a file over its baseline keeps every error live, the
//!   overage named; a file under it reports that it can tighten.
//! - `record` / `tighten`: the ledger moves only by these two — record
//!   freezes today's counts (adoption), tighten lowers each to what is
//!   left (never raises: growth is a fix or a deliberate re-record).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::enforcement::violation::{Severity, Violation};
use crate::schema::registers::ledgers::baselines::Baselines;

/// Errors one baseline absorbed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Baselined {
    pub check: String,
    pub file: String,
    pub found: usize,
    pub allowed: usize,
}

/// Error counts per (check, file).
pub fn error_counts(violations: &[Violation]) -> BTreeMap<(String, String), usize> {
    let mut counts = BTreeMap::new();
    for v in violations.iter().filter(|v| v.severity == Severity::Error) {
        *counts.entry((v.check.clone(), v.file.clone())).or_default() += 1;
    }
    counts
}

/// (live findings, baselined receipts). Receipts include baselines that
/// found fewer errors than allowed (found < allowed: tighten).
pub fn partition(
    ledger: &Baselines,
    violations: Vec<Violation>,
) -> (Vec<Violation>, Vec<Baselined>) {
    if ledger.is_empty() {
        return (violations, Vec::new());
    }
    let counts = error_counts(&violations);
    let mut live = Vec::with_capacity(violations.len());
    for mut v in violations {
        // an old ledger entry for these absorbs nothing (0.9.2)
        if v.severity == Severity::Error && !NEVER_BASELINED.contains(&v.check.as_str()) {
            let key = (v.check.clone(), v.file.clone());
            if let Some(allowed) = ledger.allowed(&v.check, &v.file) {
                let found = counts[&key];
                if found <= allowed {
                    continue; // absorbed: a receipt below says so
                }
                v.message
                    .push_str(&format!(" — over baseline: {found} > {allowed} allowed"));
            }
        }
        live.push(v);
    }
    let receipts = ledger
        .entries
        .iter()
        .filter_map(|e| {
            let found = counts
                .get(&(e.check.clone(), e.file.clone()))
                .copied()
                .unwrap_or(0);
            (found <= e.count).then(|| Baselined {
                check: e.check.clone(),
                file: e.file.clone(),
                found,
                allowed: e.count,
            })
        })
        .collect();
    (live, receipts)
}

/// Freeze today's error counts (optionally one check) into the ledger.
/// Returns how many (check, file) entries it set.
/// Errors a baseline never absorbs (0.9.2, codex field test: one
/// `baseline record` froze a contract break and the open parity debts, and
/// validate passed): a breaking change, a debt owed, data harna cannot read.
pub const NEVER_BASELINED: &[&str] = &["contract", "parity-open", "harna-data"];

pub fn record(
    ledger: &mut Baselines,
    violations: &[Violation],
    check: Option<&str>,
    today: &str,
) -> usize {
    let mut n = 0;
    for ((c, f), count) in error_counts(violations) {
        if NEVER_BASELINED.contains(&c.as_str()) {
            continue;
        }
        if check.is_none_or(|want| want == c) {
            ledger.set(&c, &f, count, today);
            n += 1;
        }
    }
    n
}

/// Lower every entry to what is left; returns (entries lowered, entries
/// paid off). Never raises an entry.
pub fn tighten(ledger: &mut Baselines, violations: &[Violation], today: &str) -> (usize, usize) {
    let counts = error_counts(violations);
    let (mut lowered, mut paid) = (0, 0);
    for e in ledger.entries.clone() {
        let found = counts
            .get(&(e.check.clone(), e.file.clone()))
            .copied()
            .unwrap_or(0);
        if found < e.count {
            if found == 0 {
                paid += 1;
            } else {
                lowered += 1;
            }
            ledger.set(&e.check, &e.file, found, today);
        }
    }
    (lowered, paid)
}

/// `harna baseline <action>` for CLI and MCP alike: `record` freezes
/// today's errors (optionally one check), `tighten` lowers to what is
/// left, `show` lists the ledger. Record and tighten read a full validate
/// run, before any ledger applies.
pub fn command(
    root: &std::path::Path,
    store: &crate::schema::store::Store,
    action: &str,
    check: Option<&str>,
) -> anyhow::Result<String> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut ledger = store.load_baselines()?;
    match action {
        "record" => {
            let found = super::run(root, store, crate::search::digest::MAX_FILES)?;
            let n = record(&mut ledger, &found, check, &today);
            store.save_baselines(&ledger)?;
            let held: usize = found
                .iter()
                .filter(|v| {
                    v.severity == super::Severity::Error
                        && NEVER_BASELINED.contains(&v.check.as_str())
                })
                .count();
            let mut msg = format!(
                "baseline recorded: {n} (check, file) entr(ies), {} error(s) allowed in all — growth now fails, `harna baseline tighten` as they are fixed\n",
                ledger.total()
            );
            if held > 0 {
                msg.push_str(&format!(
                    "  not baselined: {held} error(s) of {} — a breaking change, an owed debt or unreadable data is resolved, never frozen\n",
                    NEVER_BASELINED.join(", ")
                ));
            }
            Ok(msg)
        }
        "tighten" => {
            let found = super::run(root, store, crate::search::digest::MAX_FILES)?;
            let (lowered, paid) = tighten(&mut ledger, &found, &today);
            store.save_baselines(&ledger)?;
            Ok(format!(
                "baseline tightened: {lowered} lowered, {paid} paid off; {} error(s) still allowed\n",
                ledger.total()
            ))
        }
        "show" if ledger.is_empty() => Ok("no baselines recorded\n".into()),
        "show" => {
            let mut out = format!(
                "{} baseline(s), {} error(s) allowed:\n",
                ledger.entries.len(),
                ledger.total()
            );
            for e in &ledger.entries {
                out.push_str(&format!(
                    "  [{}] {} — {} (since {})\n",
                    e.check, e.file, e.count, e.recorded
                ));
            }
            Ok(out)
        }
        other => anyhow::bail!("unknown baseline action `{other}` — record | tighten | show"),
    }
}

#[cfg(test)]
mod tests;
