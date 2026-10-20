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
    crate::SCHEMA_BASELINE
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
    /// Milestone-keyed specs are graded through `reached` — the roadmap's
    /// verdict on the named milestone; a spec naming nothing declared is
    /// reached by no id (the ghost is the coherence check's finding).
    ///
    /// `version` is the PROJECT's version (0.9.0: harna passed its own, so
    /// `review = "1.2.0"` waited for harna 1.2.0); `declared` says which
    /// ids the roadmap declares, so a version-shaped milestone id
    /// (`0.9.0`) is graded as the milestone.
    pub fn overdue(
        &self,
        version: &str,
        today: &str,
        reached: &dyn Fn(&str) -> bool,
        declared: &dyn Fn(&str) -> bool,
    ) -> Vec<&DeferredEntry> {
        self.entries
            .iter()
            .filter(|e| is_due(&e.review, version, today, reached, declared))
            .collect()
    }
}

/// The kind of review trigger an entry carries: an ISO date
/// (`2027-01-15` — ISO sorts lexicographically), a version the tool
/// must reach (`0.4.0`), or a MILESTONE id named in the roadmap
/// (`.harna/roadmap.toml`, 0.3.28) — anything with neither date nor
/// version syntax. An unpinnable spec cannot demand a review on its
/// own; the entry still surfaces in counts, and the roadmap coherence
/// check names what an undeclared milestone id points at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReviewKind {
    IsoDate,
    Version,
    /// No date syntax, no version syntax — a roadmap milestone id.
    Milestone,
}

/// The review spec without its explicit kind prefix (0.9.0: `date:`,
/// `version:`, `milestone:` say what a spec is when its shape cannot).
pub fn review_target(review: &str) -> &str {
    let r = review.trim();
    for p in ["date:", "version:", "milestone:"] {
        if let Some(rest) = r.strip_prefix(p) {
            return rest.trim();
        }
    }
    r
}

/// The kind with the roadmap's word on bare version-shaped specs: one
/// naming a declared milestone is that milestone (0.9.0: harna's own ids
/// look like versions and could never trigger as milestones).
pub fn kind_in(review: &str, declared: &dyn Fn(&str) -> bool) -> ReviewKind {
    let r = review.trim();
    let explicit = ["date:", "version:", "milestone:"]
        .iter()
        .any(|p| r.starts_with(p));
    match review_kind(r) {
        ReviewKind::Version if !explicit && declared(r) => ReviewKind::Milestone,
        k => k,
    }
}

/// Is a review due: by date, by the project's version, or by the roadmap.
pub fn is_due(
    review: &str,
    version: &str,
    today: &str,
    reached: &dyn Fn(&str) -> bool,
    declared: &dyn Fn(&str) -> bool,
) -> bool {
    match kind_in(review, declared) {
        ReviewKind::Milestone => milestone_due(review, reached),
        _ => review_due(review, version, today),
    }
}

/// Classify a review spec: an explicit prefix says it; otherwise dates
/// first (dates also read as ids and must win), versions second,
/// everything else a milestone id.
pub fn review_kind(review: &str) -> ReviewKind {
    let raw = review.trim();
    if raw.starts_with("date:") {
        return ReviewKind::IsoDate;
    }
    if raw.starts_with("version:") {
        return ReviewKind::Version;
    }
    if raw.starts_with("milestone:") {
        return ReviewKind::Milestone;
    }
    let review = raw;
    let b = review.as_bytes();
    if review.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().all(|c| c.is_ascii_digit() || *c == b'-')
    {
        return ReviewKind::IsoDate;
    }
    if parse_version(review).is_some() {
        return ReviewKind::Version;
    }
    ReviewKind::Milestone
}

/// Is this entry's review trigger arrived by CLOCK? Dates are due on
/// the day they name; versions once the running tool reaches them; a
/// milestone-keyed spec is never due by clock — the register grades it
/// through the roadmap instead (`overdue` takes `reached`).
pub fn review_due(review: &str, current_version: &str, today: &str) -> bool {
    let target = review_target(review);
    match review_kind(review) {
        ReviewKind::IsoDate => target <= today,
        ReviewKind::Version => match (parse_version(current_version), parse_version(target)) {
            (Some(c), Some(r)) => c >= r,
            _ => false,
        },
        ReviewKind::Milestone => false,
    }
}

/// A milestone-keyed review's due state, given the roadmap's verdict on
/// whether the named milestone has been reached (state = done).
pub fn milestone_due(review: &str, reached: &dyn Fn(&str) -> bool) -> bool {
    let target = review_target(review);
    !target.is_empty() && reached(target)
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
        assert!(
            review_due("2026-10-16", "0.3.23", "2026-10-17"),
            "yesterday is overdue"
        );
        assert!(
            review_due("2026-10-17", "0.3.23", "2026-10-17"),
            "the day it names, it is owed"
        );
        assert!(!review_due("2027-01-15", "0.3.23", "2026-10-17"));
        // Versions: due once the running tool reaches the named one.
        assert!(
            !review_due("0.4.0", "0.3.23", "2026-10-17"),
            "not reached yet"
        );
        assert!(
            review_due("0.4", "0.4.9", "2026-10-17"),
            "0.4 due at 0.4.9 (padding)"
        );
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
        r.upsert(entry("a", "2099-01-01")); // replaced, never duplicated
        assert_eq!(r.entries.len(), 3);
        assert!(!r.remove("nope"));
        assert!(r.remove("b"));
        assert_eq!(r.entries.len(), 2);
        // Only a's *previous* overdue spec left; it was replaced.
        assert!(r
            .overdue("0.3.23", "2026-10-17", &|_| false, &|_| false)
            .is_empty());
        r.upsert(entry("a", "2026-10-10"));
        assert_eq!(
            r.overdue("0.3.23", "2026-10-17", &|_| false, &|_| false)
                .len(),
            1
        );
    }

    #[test]
    fn reviews_classify_date_version_milestone() {
        assert_eq!(review_kind("2027-01-15"), ReviewKind::IsoDate);
        assert_eq!(review_kind("0.4.0"), ReviewKind::Version);
        assert_eq!(review_kind("0.4"), ReviewKind::Version);
        assert_eq!(review_kind(" phase-h "), ReviewKind::Milestone);
        assert_eq!(review_kind(""), ReviewKind::Milestone);
        assert_eq!(review_kind("eventually"), ReviewKind::Milestone);
        // Dates win over ids: a date-shaped spec is a date.
        assert_eq!(review_kind("2027-01-01"), ReviewKind::IsoDate);
        // A milestone review is clock-blind...
        assert!(!review_due("phase-h", "9.9.9", "2099-12-31"));
        // ...and due exactly when the roadmap declares the id reached.
        assert!(milestone_due("phase-h", &|id| id == "phase-h"));
        assert!(
            !milestone_due("phase-h", &|_| false),
            "declared but not done"
        );
        assert!(!milestone_due("", &|_| true), "empty spec, no id to reach");
    }

    #[test]
    fn milestone_keyed_reviews_grade_through_reached() {
        let mut r = DeferredRegister::default();
        r.upsert(entry("m-keyed", "roadmap-cleanup"));
        r.upsert(entry("m-ghost", "undeclared-milestone"));
        // Declared + done: due. Declared or not, never clock-due.
        let reached = |id: &str| id == "roadmap-cleanup";
        let names = r
            .overdue("9.9.9", "2099-12-31", &reached, &|_| true)
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["m-keyed"]);
        assert!(r
            .overdue("9.9.9", "2099-12-31", &|_| false, &|_| true)
            .is_empty());
    }
}

#[cfg(test)]
mod trigger_tests {
    use super::*;

    /// 0.9.0: an explicit prefix says what a spec is; a bare version-shaped
    /// spec naming a declared milestone is that milestone; versions grade
    /// against the project's version.
    #[test]
    fn explicit_and_roadmap_aware_triggers() {
        let declared = |id: &str| id == "0.9.0";
        assert_eq!(kind_in("0.9.0", &declared), ReviewKind::Milestone);
        assert_eq!(kind_in("version:0.9.0", &declared), ReviewKind::Version);
        assert_eq!(kind_in("1.2.0", &declared), ReviewKind::Version);
        assert_eq!(review_kind("milestone:beta"), ReviewKind::Milestone);
        assert_eq!(review_kind("date:2027-01-15"), ReviewKind::IsoDate);
        assert!(is_due(
            "0.9.0",
            "0.1.0",
            "2026-01-01",
            &|id| id == "0.9.0",
            &declared
        ));
        assert!(!is_due(
            "version:0.9.0",
            "0.1.0",
            "2026-01-01",
            &|_| true,
            &declared
        ));
        assert!(is_due(
            "version:0.9.0",
            "0.9.1",
            "2026-01-01",
            &|_| false,
            &declared
        ));
        assert!(is_due(
            "date:2026-01-01",
            "",
            "2026-01-01",
            &|_| false,
            &declared
        ));
    }
}
