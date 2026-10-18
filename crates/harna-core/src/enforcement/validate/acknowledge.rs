//! Component `acknowledge` (0.3.31): the advisory ledger's enforcement
//! half — pure partition logic, no filesystem.
//!
//! `validate` reports every finding; this module splits Info findings
//! into *live* and *locked*: an Info covered by the advisory ledger
//! (`.harna/acknowledged.toml`) rides the acknowledged list with its
//! recorded reason, everything else stays live. Errors are never
//! acknowledgeable — a covered error passes straight through, so a
//! locked advisory can never smuggle a gate failure past the commit
//! hook. Callers render the acknowledged list as one-line receipts and
//! keep them out of strict-mode gating.

use crate::enforcement::violation::{Severity, Violation};
use crate::schema::registers::acknowledged::Acknowledged;
use serde::Serialize;

/// An Info finding the ledger covers, with the reason it was accepted.
#[derive(Debug, Clone, Serialize)]
pub struct AckedFinding {
    pub violation: Violation,
    pub reason: String,
}

/// Split findings: (live, acknowledged). Errors are never lockable —
/// even a ledger-covered error stays live.
pub fn partition(
    ledger: &Acknowledged,
    violations: Vec<Violation>,
) -> (Vec<Violation>, Vec<AckedFinding>) {
    let mut live = Vec::new();
    let mut acked = Vec::new();
    for v in violations {
        if v.severity == Severity::Info {
            if let Some(a) = ledger.covers(&v.check, &v.file) {
                acked.push(AckedFinding {
                    violation: v,
                    reason: a.reason.clone(),
                });
                continue;
            }
        }
        live.push(v);
    }
    (live, acked)
}

/// The conformance verdict every surface reports (CLI, MCP, preflight):
/// live findings, receipts, counts, and whether the gate blocks. One
/// computation, so no surface can disagree on what blocks.
#[derive(Debug, Clone, Serialize)]
pub struct GateReport {
    pub violations: Vec<Violation>,
    pub acknowledged: Vec<AckedFinding>,
    pub errors: usize,
    pub infos: usize,
    /// Errors always block; Infos block too under `strict`.
    pub blocks: bool,
}

pub fn gate(ledger: &Acknowledged, violations: Vec<Violation>, strict: bool) -> GateReport {
    let (violations, acknowledged) = partition(ledger, violations);
    let errors = violations.iter().filter(|v| v.severity == Severity::Error).count();
    let infos = violations.len() - errors;
    GateReport {
        blocks: errors > 0 || (strict && infos > 0),
        violations,
        acknowledged,
        errors,
        infos,
    }
}

impl GateReport {
    /// Findings, receipts, the count line and — always — the verdict, so
    /// a reader (human or agent) never has to infer what blocks.
    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.violations.is_empty() && self.acknowledged.is_empty() {
            out.push_str("no violations found\n");
        } else {
            for v in &self.violations {
                out.push_str(&v.render());
                out.push('\n');
            }
            for a in &self.acknowledged {
                out.push_str(&format!(
                    "ack [{}] {} — {}\n",
                    a.violation.check, a.violation.file, a.reason
                ));
            }
            out.push_str(&format!(
                "\n{} violation(s) ({} error, {} info, {} acknowledged)\n",
                self.violations.len(),
                self.errors,
                self.infos,
                self.acknowledged.len()
            ));
        }
        out.push_str(if self.blocks {
            "verdict: BLOCKED — fix the errors (or the infos, in strict mode) before committing\n"
        } else {
            "verdict: PASS — nothing blocks\n"
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_states_the_verdict_and_honours_the_ledger_and_strict() {
        let ledger = ledger_for("no-copy-paste", "a.rs");
        let findings = || {
            vec![
                finding("no-copy-paste", "a.rs", Severity::Info),
                finding("hotspot", "b.rs", Severity::Info),
            ]
        };
        let lax = gate(&ledger, findings(), false);
        assert_eq!((lax.errors, lax.infos, lax.acknowledged.len()), (0, 1, 1));
        assert!(!lax.blocks, "infos never block outside strict mode");
        assert!(lax.render().contains("verdict: PASS"));
        assert!(lax.render().contains("ack [no-copy-paste] a.rs"));

        let strict = gate(&ledger, findings(), true);
        assert!(strict.blocks, "a live info blocks in strict mode");
        assert!(strict.render().contains("verdict: BLOCKED"));
        // The acknowledged info stays out of strict gating.
        let only_acked = gate(&ledger, vec![finding("no-copy-paste", "a.rs", Severity::Info)], true);
        assert!(!only_acked.blocks);

        let err = gate(&ledger, vec![finding("no-copy-paste", "a.rs", Severity::Error)], false);
        assert!(err.blocks, "errors are never acknowledgeable");
        assert!(gate(&ledger, vec![], false).render().starts_with("no violations found"));
    }

    fn finding(check: &str, file: &str, severity: Severity) -> Violation {
        Violation {
            check: check.into(),
            file: file.into(),
            line: 1,
            message: "m".into(),
            hint: "h".into(),
            severity,
        }
    }

    fn ledger_for(check: &str, file: &str) -> Acknowledged {
        let mut a = Acknowledged::default();
        a.upsert(crate::schema::registers::acknowledged::Acknowledgement {
            check: check.into(),
            file: file.into(),
            reason: "tracked elsewhere".into(),
            recorded: "2026-10-17".into(),
        });
        a
    }

    #[test]
    fn covered_infos_lock_uncovered_stay_live() {
        let ledger = ledger_for("hotspot", "big.rs");
        let (live, acked) = partition(
            &ledger,
            vec![
                finding("hotspot", "big.rs", Severity::Info),
                finding("hotspot", "other.rs", Severity::Info),
            ],
        );
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].file, "other.rs");
        assert_eq!(acked.len(), 1);
        assert_eq!(acked[0].reason, "tracked elsewhere");
    }

    #[test]
    fn errors_are_never_lockable() {
        let ledger = ledger_for("layer-boundary", "bad.rs");
        let (live, acked) = partition(
            &ledger,
            vec![finding("layer-boundary", "bad.rs", Severity::Error)],
        );
        assert_eq!(live.len(), 1, "a covered error stays live — gates are not suggestions");
        assert!(acked.is_empty());
    }

    #[test]
    fn empty_ledger_changes_nothing() {
        let (live, acked) = partition(
            &Acknowledged::default(),
            vec![finding("hotspot", "big.rs", Severity::Info)],
        );
        assert_eq!(live.len(), 1);
        assert!(acked.is_empty());
    }
}
