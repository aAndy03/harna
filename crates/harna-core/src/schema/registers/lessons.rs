//! Component `lessons-data` (0.3.27): the DEAD-ENDS register's data
//! structs live in the memory layer — same discipline as `terms-data`
//! and `non-goals-data`: the store persists the document, the pure
//! logic (hay matching, upsert/remove) is testable without a
//! filesystem.
//!
//! A lesson records a TRIED-AND-FAILED direction: the claim (what was
//! attempted), the verdict (why it did not work out), and evidence
//! pointers (decisions, commits) so the next session can verify rather
//! than take it on faith. This is the audit of failure the decision
//! log's supersession chains could not carry alone: superseded
//! decisions say what the NEW answer is; lessons say what NOT to
//! suggest again. `place`/`plan` mine them into a "prior attempts"
//! block so a fresh agent meets the dead ends before walking into them.

use serde::{Deserialize, Serialize};

/// Version of the dead-ends register document (`.harna/lessons.toml`).
fn current() -> u32 {
    crate::SCHEMA_VERSION
}

/// Top-level dead-ends register (`.harna/lessons.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lessons {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lessons: Vec<Lesson>,
}

/// One tried-and-failed record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lesson {
    /// Stable id, e.g. "ps-roundtrip-source-files".
    pub id: String,
    /// The claim — what was attempted, in the direction of the failure.
    pub claim: String,
    /// The verdict — why it did not work out.
    pub verdict: String,
    /// Evidence pointers: decision files, commits, events. Verifiable,
    /// not testimonial — an agent re-checks before re-suggesting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    /// When it was recorded (ISO date, filled by the CLI).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recorded: String,
}

impl Lesson {
    /// The search hay: claim + verdict + evidence, lowercased — the
    /// same matching surface `place`/`plan` tokenize against.
    pub fn hay(&self) -> String {
        format!(
            "{} {} {}",
            self.claim,
            self.verdict,
            self.evidence.join(" ")
        )
        .to_lowercase()
    }
}

impl Lessons {
    pub fn is_empty(&self) -> bool {
        self.lessons.is_empty()
    }

    /// Add or replace by id (case-insensitive; a re-recording is a
    /// refined post-mortem of the same failure).
    pub fn upsert(&mut self, entry: Lesson) {
        self.lessons
            .retain(|l| !l.id.eq_ignore_ascii_case(&entry.id));
        self.lessons.push(entry);
        self.lessons.sort_by(|a, b| a.id.cmp(&b.id));
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.lessons.len();
        self.lessons
            .retain(|l| !l.id.eq_ignore_ascii_case(id));
        before != self.lessons.len()
    }

    /// Lessons overlapping the idea's tokens, strongest overlap first,
    /// capped at `max` — the "prior attempts" mining for briefings.
    pub fn prior_to<'a>(&'a self, tokens: &[String], max: usize) -> Vec<(&'a Lesson, usize)> {
        let mut scored: Vec<(&Lesson, usize)> = self
            .lessons
            .iter()
            .map(|l| {
                let hay = l.hay();
                let hits = tokens.iter().filter(|t| hay.contains(t.as_str())).count();
                (l, hits)
            })
            .filter(|(_, hits)| *hits > 0)
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.id.cmp(&b.0.id)));
        scored.truncate(max);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lesson(id: &str, claim: &str, verdict: &str) -> Lesson {
        Lesson {
            id: id.into(),
            claim: claim.into(),
            verdict: verdict.into(),
            evidence: vec![],
            recorded: "2026-10-17".into(),
        }
    }

    fn tokens(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn upsert_by_id_is_case_insensitive_and_sorted() {
        let mut l = Lessons::default();
        assert!(l.is_empty());
        l.upsert(lesson("delta", "c", "v"));
        l.upsert(lesson("axe", "c", "v"));
        l.upsert(lesson("DELTA", "c refined", "v"));
        assert_eq!(l.lessons.len(), 2);
        assert_eq!(
            l.lessons[0].id, "DELTA",
            "byte-wise sort: uppercase 'D' precedes lowercase 'a'"
        );
        assert_eq!(l.lessons[0].claim, "c refined");
        assert_eq!(l.lessons[1].id, "axe");
    }

    #[test]
    fn hay_is_lowercase_across_claim_verdict_evidence() {
        let mut l = Lessons::default();
        let mut e = lesson("ps-roundtrip", "Bulk-edit source files via PowerShell", "CP1252 misdecode Corrupted Files");
        e.evidence = vec!["commit d4e81ed".into()];
        l.upsert(e);
        let hay = l.lessons[0].hay();
        assert!(hay.contains("bulk-edit"));
        assert!(hay.contains("corrupted"));
        assert!(hay.contains("commit d4e81ed"), "evidence rides the hay");
    }

    #[test]
    fn prior_to_ranks_strongest_overlap_first_and_caps() {
        let mut l = Lessons::default();
        l.upsert(lesson("weak", "nothing about this idea", "irrelevant"));
        l.upsert(lesson("one-hit", "windows files", "reasoning vague, asserts lost"));
        let mut strong = lesson("two-hit", "encoding windows files", "cp1252 broke encoding");
        strong.evidence = vec!["windows incident".into()];
        l.upsert(strong);

        let ranked = l.prior_to(&tokens(&["encoding", "windows", "cp1252"]), 2);
        assert_eq!(ranked.len(), 2, "capped at 2, the no-overlap one is out");
        assert_eq!(ranked[0].0.id, "two-hit", "3 hits beat 2");
        assert_eq!(ranked[0].1, 3);
        assert_eq!(ranked[1].0.id, "one-hit");
    }

    #[test]
    fn remove_is_case_insensitive_and_honest() {
        let mut l = Lessons::default();
        l.upsert(lesson("gone", "c", "v"));
        assert!(l.remove("GONE"));
        assert!(l.is_empty());
        assert!(!l.remove("gone"));
    }
}
