//! Component `deferred-data` (0.3.23): the DEBT REGISTER's data structs
//! live in the memory layer — same discipline as `digest-data` and
//! `parity-data`: the store persists the document, data owned by the
//! store belongs next to it, and the behavior (due-checking, counting)
//! is pure data logic testable without a filesystem.
//!
//! A deferred register entry records a DELIBERATE deferral: what would
//! need touching when it is collected (component), why it was deferred
//! (reason), and when it must be revisited (review — an ISO date or a
//! version). This is the ledger for standing "no ask yet" notes that used
//! to live inside decision paragraphs; it is deliberately distinct from
//! parity debts (structural follow-through) — these are promises to
//! reconsider, not unresolved change propagation.

use serde::{Deserialize, Serialize};

/// Version of the register document (`.harna/deferred.toml`).
fn current() -> u32 {
    crate::SCHEMA_VERSION
}

/// Top-level register document (`.harna/deferred.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeferredRegister {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<DeferredEntry>,
}

/// One deliberate deferral, awaiting its review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeferredEntry {
    /// Stable id, e.g. "fixture-builder-dedup".
    pub id: String,
    /// What collecting this deferral would touch (component name, module
    /// or container path — free-form, the register is a map's pointer,
    /// not a manifest validator).
    pub component: String,
    /// Why the deferral is deliberate, not an accident.
    pub reason: String,
    /// When to revisit: an ISO date (`2027-01-15`) or a version the tool
    /// must reach (`0.4.0`).
    pub review: String,
    /// When the entry was recorded (ISO date, filled by the CLI).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recorded: String,
}

impl DeferredRegister {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Add or replace by id (a collected-and-re-recorded deferral is the
    /// same promise, freshly reasoned).
    pub fn upsert(&mut self, entry: DeferredEntry) {
        self.entries.retain(|e| e.id != entry.id);
        self.entries.push(entry);
        self.entries.sort_by(|a, b| a.id.cmp(&b.id));
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        before != self.entries.len()
    }

    /// Entries whose review trigger has arrived (status + score surface).
    pub fn overdue(&self, current_version: &str, today: &str) -> Vec<&DeferredEntry> {
        self.entries
            .iter()
            .filter(|e| review_due(&e.review, current_version, today))
            .collect()
    }
}

/// Is this entry's review trigger arrived? A review spec is either an ISO
/// date (`2027-01-15` — ISO sorts lexicographically) or a version the tool
/// must reach (`0.4.0`). Anything unparseable is never due — an unpinnable
/// spec cannot demand a review; the entry still surfaces in the count.
pub fn review_due(review: &str, current_version: &str, today: &str) -> bool {
    let review = review.trim();
    let b = review.as_bytes();
    if review.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().all(|c| c.is_ascii_digit() || *c == b'-') {
        return review <= today;
    }
    match (parse_version(current_version), parse_version(review)) {
        (Some(c), Some(r)) => c >= r,
        _ => false,
    }
}

/// Numeric version key padded to four components ("0.4" == "0.4.0").
fn parse_version(s: &str) -> Option<(u16, u16, u16, u16)> {
    let raw: Vec<u16> = s
        .trim()
        .split('.')
        .map(|p| p.parse::<u16>().ok())
        .collect::<Option<Vec<_>>>()?;
    let at = |i: usize| raw.get(i).copied().unwrap_or(0);
    Some((at(0), at(1), at(2), at(3)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, review: &str) -> DeferredEntry {
        DeferredEntry {
            id: id.into(),
            component: "x".into(),
            reason: "deliberate".into(),
            review: review.into(),
            recorded: "2026-10-17".into(),
        }
    }

    #[test]
    fn review_dates_sort_lexicographically_and_versions_arrive() {
        // ISO dates: strictly string-ordered. The trigger day itself is
        // DUE — a review date arriving means the promise is owed now.
        assert!(review_due("2026-10-16", "0.3.23", "2026-10-17"), "yesterday is overdue");
        assert!(review_due("2026-10-17", "0.3.23", "2026-10-17"), "the day it names, it is owed");
        assert!(!review_due("2027-01-15", "0.3.23", "2026-10-17"));
        // Versions: due once the running tool reaches the named one.
        assert!(!review_due("0.4.0", "0.3.23", "2026-10-17"), "not reached yet");
        assert!(review_due("0.4", "0.4.9", "2026-10-17"), "0.4 due at 0.4.9 (padding)");
        assert!(review_due("0.3.23", "0.3.23", "2026-10-17"));
        // Unparseable: never due.
        assert!(!review_due("eventually", "0.3.23", "2026-10-17"));
        assert!(!review_due("", "0.3.23", "2026-10-17"));
    }

    #[test]
    fn register_upserts_removes_and_counts_overdue() {
        let mut r = DeferredRegister::default();
        assert!(r.is_empty());
        r.upsert(entry("a", "2026-01-01"));
        r.upsert(entry("b", "9.9.9"));
        r.upsert(entry("c", "2099-01-01"));
        r.upsert(entry("a", "2099-01-01"), ); // replaced, never duplicated
        assert_eq!(r.entries.len(), 3);
        assert!(!r.remove("nope"));
        assert!(r.remove("b"));
        assert_eq!(r.entries.len(), 2);
        // Only a's *previous* overdue spec left; it was replaced.
        assert!(r.overdue("0.3.23", "2026-10-17").is_empty());
        r.upsert(entry("a", "2026-10-10"));
        assert_eq!(r.overdue("0.3.23", "2026-10-17").len(), 1);
    }
}
