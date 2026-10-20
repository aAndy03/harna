//! Component `non-goals-data` (0.3.27): the DECLINE register's data
//! structs live in the memory layer — same discipline as `terms-data`:
//! the store persists the document, the pure logic (token matching,
//! upsert/remove) is testable without a filesystem.
//!
//! A non-goal records a DELIBERATE product refusal: what the app, the
//! layer or harna itself does NOT do, and why the refusal was made.
//! This is deliberately distinct from `rules.toml`: a rule is a code
//! convention a check can police; a non-goal is a product decision the
//! agent must not *propose around* — `plan` suppresses matching
//! proposals and names the supersession path (removal requires the
//! user's agreement, recorded in the decision log). One refusal per
//! idea, upserted by id; removal is data change, git keeps history.

use serde::{Deserialize, Serialize};

/// Version of the decline register document (`.harna/non-goals.toml`).
fn current() -> u32 {
    crate::SCHEMA_BASELINE
}

/// Top-level decline register (`.harna/non-goals.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NonGoals {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub goals: Vec<NonGoal>,
}

/// One deliberate refusal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NonGoal {
    /// Stable id, e.g. "no-remote-state".
    pub id: String,
    /// The refusal in imperative form, e.g. "harna does not probe live
    /// production systems".
    pub refusal: String,
    /// Why it was refused — the supersession briefing's evidence.
    pub reason: String,
    /// When it was recorded (ISO date, filled by the CLI).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recorded: String,
}

impl NonGoals {
    pub fn is_empty(&self) -> bool {
        self.goals.is_empty()
    }

    /// Add or replace by id (case-insensitive; a re-recording refines
    /// the same refusal, it does not multiply it).
    pub fn upsert(&mut self, entry: NonGoal) {
        self.goals.retain(|g| !g.id.eq_ignore_ascii_case(&entry.id));
        self.goals.push(entry);
        self.goals.sort_by(|a, b| a.id.cmp(&b.id));
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.goals.len();
        self.goals.retain(|g| !g.id.eq_ignore_ascii_case(id));
        before != self.goals.len()
    }

    /// Non-goals whose refusal or reason mentions any of the given
    /// (already-lowercased, generic-free) tokens — the "do not propose"
    /// block for `place`/`plan`. One token hit is enough: refusals are
    /// short and explicit, so vague over-matching has to be bought by
    /// vague refusals, not loose matching.
    pub fn matching<'a>(&'a self, tokens: &[String]) -> Vec<&'a NonGoal> {
        self.goals
            .iter()
            .filter(|g| {
                let hay = format!("{} {}", g.refusal, g.reason).to_lowercase();
                tokens.iter().any(|t| hay.contains(t.as_str()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goal(id: &str, refusal: &str, reason: &str) -> NonGoal {
        NonGoal {
            id: id.into(),
            refusal: refusal.into(),
            reason: reason.into(),
            recorded: "2026-10-17".into(),
        }
    }

    fn tokens(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn upsert_by_id_is_case_insensitive_and_sorted() {
        let mut g = NonGoals::default();
        assert!(g.is_empty());
        g.upsert(goal("alpha", "no A", "r"));
        g.upsert(goal("beta", "no B", "r"));
        g.upsert(goal("ALPHA", "no A refined", "r"));
        assert_eq!(g.goals.len(), 2, "mixed-case id replaces, not duplicates");
        assert!(
            g.goals[0].id.eq_ignore_ascii_case("alpha"),
            "refusal kept as written: {}",
            g.goals[0].id
        );
        assert_eq!(g.goals[0].refusal, "no A refined");
    }

    #[test]
    fn remove_is_case_insensitive_and_honest() {
        let mut g = NonGoals::default();
        g.upsert(goal("no-remote", "x", "y"));
        assert!(g.remove("NO-Remote"));
        assert!(g.is_empty(), "one removal emptied it");
        assert!(!g.remove("never-existed"));
    }

    #[test]
    fn matching_surfaces_any_token_hit_across_refusal_and_reason() {
        let mut g = NonGoals::default();
        g.upsert(goal(
            "no-remote-state",
            "harna does not probe live production systems",
            "production awareness is a read-only surface in 0.4",
        ));
        g.upsert(goal(
            "no-editor-control",
            "harna never takes over the editor",
            "the agent stays the author",
        ));
        let hit = g.matching(&tokens(&["production", "hallucinated"]));
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].id, "no-remote-state");
        assert!(
            g.matching(&tokens(&["hallucinated"])).is_empty(),
            "no overlap means no block"
        );
        assert!(g.matching(&tokens(&[])).is_empty(), "no tokens, no matches");
    }
}
