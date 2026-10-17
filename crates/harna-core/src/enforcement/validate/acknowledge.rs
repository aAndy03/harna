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
use crate::schema::acknowledged_data::Acknowledged;
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

#[cfg(test)]
mod tests {
    use super::*;

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
        a.upsert(crate::schema::acknowledged_data::Acknowledgement {
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
