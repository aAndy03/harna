//! Component `acknowledged-data` (0.3.31): the ADVISORY LEDGER's data
//! structs live in the memory layer — the store persists the document,
//! the pure logic (upsert/remove/match) is testable without a
//! filesystem.
//!
//! An acknowledgment records an ACCEPTED advisory: a specific Info
//! finding (check + file) the human has seen, reasoned about, and
//! locked away with a why. Errors can never be acknowledged — a gate
//! is not a suggestion. Matching is line-agnostic (lines shift; the
//! finding's identity is check + file), and removal re-arms the
//! advisory. `validate` renders acknowledged findings as one-line
//! `ack` receipts instead of full blocks and excludes them from
//! strict-mode gating: locked means locked, but still visible.

use serde::{Deserialize, Serialize};

/// Version of the advisory ledger document (`.harna/acknowledged.toml`).
fn current() -> u32 {
    crate::SCHEMA_BASELINE
}

/// Top-level advisory ledger (`.harna/acknowledged.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Acknowledged {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<Acknowledgement>,
}

/// One locked-away advisory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Acknowledgement {
    /// The check that fired, e.g. "no-copy-paste".
    pub check: String,
    /// The file it fired on, repo-relative, e.g. "crates/x/src/main.rs".
    pub file: String,
    /// Why the advisory is accepted rather than fixed.
    pub reason: String,
    /// When it was recorded (ISO date, filled by the CLI).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recorded: String,
}

impl Acknowledged {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Add or replace by (check, file) — a re-acknowledgment is a
    /// refreshed rationale for the same accepted advisory.
    pub fn upsert(&mut self, entry: Acknowledgement) {
        self.entries
            .retain(|e| !(e.check.eq_ignore_ascii_case(&entry.check) && e.file == entry.file));
        self.entries.push(entry);
        self.entries
            .sort_by(|a, b| a.check.cmp(&b.check).then(a.file.cmp(&b.file)));
    }

    pub fn remove(&mut self, check: &str, file: &str) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|e| !(e.check.eq_ignore_ascii_case(check) && e.file == file));
        before != self.entries.len()
    }

    /// The lock lookup: does an accepted advisory cover this finding?
    /// Check matches case-insensitively (ids are lowercase by
    /// convention, humans type otherwise); the file matches exactly
    /// (paths are case-sensitive on the filesystems harna governs).
    pub fn covers(&self, check: &str, file: &str) -> Option<&Acknowledgement> {
        self.entries
            .iter()
            .find(|e| e.check.eq_ignore_ascii_case(check) && e.file == file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ack(check: &str, file: &str) -> Acknowledgement {
        Acknowledgement {
            check: check.into(),
            file: file.into(),
            reason: "accepted: tracked elsewhere".into(),
            recorded: "2026-10-17".into(),
        }
    }

    #[test]
    fn upsert_keys_on_check_plus_file() {
        let mut a = Acknowledged::default();
        assert!(a.is_empty());
        a.upsert(ack("no-copy-paste", "a.rs"));
        a.upsert(ack("no-copy-paste", "b.rs"));
        a.upsert(ack("NO-COPY-PASTE", "a.rs"));
        assert_eq!(a.entries.len(), 2, "same check+file replaces");
        assert_eq!(a.entries[0].file, "a.rs", "sorted by check then file");
    }

    #[test]
    fn covers_is_check_case_insensitive_file_exact() {
        let mut a = Acknowledged::default();
        a.upsert(ack("hotspot", "src/Store.rs"));
        assert!(a.covers("HOTSPOT", "src/Store.rs").is_some());
        assert!(
            a.covers("hotspot", "src/store.rs").is_none(),
            "paths stay exact"
        );
        assert!(a.covers("other", "src/Store.rs").is_none());
    }

    #[test]
    fn remove_re_arms() {
        let mut a = Acknowledged::default();
        a.upsert(ack("hotspot", "x.rs"));
        assert!(a.remove("HOTSPOT", "x.rs"));
        assert!(a.is_empty());
        assert!(!a.remove("hotspot", "x.rs"));
    }
}
