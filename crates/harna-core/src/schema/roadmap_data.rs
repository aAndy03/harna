//! Component `roadmap-data` (0.3.28): the ROADMAP's data structs live
//! in the memory layer — same discipline as `non-goals-data` and
//! `lessons-data`: the store persists the document, the pure logic
//! (state vocabulary, current-milestone lookup, coherence) is testable
//! without a filesystem.
//!
//! A roadmap is DECLARED data, never generated: the human and the
//! agent agree on where the project is going, `.harna/roadmap.toml`
//! records it, and harna makes it load-bearing — onboarding and status
//! surface the current milestone, validations check its coherence, and
//! deferred-register reviews can key to a milestone instead of a date
//! or version (due when the named milestone is done). States:
//! `declared` (upcoming), `current` (the burst(s) underway), `done`
//! (shipped and tagged), `dropped` (consciously abandoned). Removal is
//! a data change; git keeps the history.

use serde::{Deserialize, Serialize};

/// Version of the roadmap document (`.harna/roadmap.toml`).
fn current() -> u32 {
    crate::SCHEMA_VERSION
}

/// Top-level roadmap (`.harna/roadmap.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Roadmap {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub milestones: Vec<Milestone>,
}

/// One milestone on the declared path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Milestone {
    /// Stable id, e.g. "0.3.28" or "phase-h" — what deferred reviews
    /// name when they key to a milestone.
    pub id: String,
    /// What the milestone delivers, in one line.
    pub title: String,
    /// One of the four states in `STATES`.
    #[serde(default = "default_state")]
    pub state: String,
    /// Optional longer goal statement.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

fn default_state() -> String {
    "declared".to_string()
}

impl Milestone {
    pub const STATES: [&'static str; 4] = ["declared", "current", "done", "dropped"];

    pub fn state_is_known(&self) -> bool {
        Self::STATES.contains(&self.state.as_str())
    }
}

impl Roadmap {
    pub fn is_empty(&self) -> bool {
        self.milestones.is_empty()
    }

    /// Look a milestone up by id (trimmed, case-insensitive — review
    /// specs quote them, humans type them).
    pub fn find(&self, id: &str) -> Option<&Milestone> {
        self.milestones
            .iter()
            .find(|m| m.id.eq_ignore_ascii_case(id.trim()))
    }

    /// The milestone currently underway — the one line onboarding and
    /// status surface. More than one `current` is a coherence error,
    /// so the first hit is already a bug worth naming.
    pub fn current(&self) -> Option<&Milestone> {
        let mut hit = self.milestones.iter().filter(|m| m.state == "current");
        let first = hit.next()?;
        let _extra_exists = hit.next().is_some();
        Some(first)
    }

    /// Whether a milestone id has been reached — the verdict deferred
    /// reviews grade against when they key to a milestone.
    pub fn reached(&self, id: &str) -> bool {
        self.find(id).is_some_and(|m| m.state == "done")
    }

    /// Add or replace by id (case-insensitive; a re-recording is a
    /// clarified statement of the same destination).
    pub fn upsert(&mut self, entry: Milestone) {
        self.milestones
            .retain(|m| !m.id.eq_ignore_ascii_case(&entry.id));
        self.milestones.push(entry);
        self.milestones.sort_by(|a, b| a.id.to_lowercase().cmp(&b.id.to_lowercase()));
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.milestones.len();
        self.milestones
            .retain(|m| !m.id.eq_ignore_ascii_case(id.trim()));
        before != self.milestones.len()
    }

    /// Internal coherence problems: duplicate ids, unknown states,
    /// more than one `current`. Empty means the roadmap is sound data.
    pub fn coherence(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        let mut currents = 0usize;
        for m in &self.milestones {
            if seen.contains(&m.id.as_str()) {
                out.push(format!("duplicate milestone id `{}`", m.id));
            }
            seen.push(m.id.as_str());
            if !m.state_is_known() {
                out.push(format!(
                    "milestone `{}` carries unrecognized state `{}` (one of: {})",
                    m.id, m.state, Milestone::STATES.join("|")
                ));
            }
            if m.state == "current" {
                currents += 1;
            }
        }
        if currents > 1 {
            out.push(format!(
                "{currents} milestones marked `current` — exactly one may be under way"
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn milestone(id: &str, state: &str) -> Milestone {
        Milestone {
            id: id.into(),
            title: format!("deliver {id}"),
            state: state.into(),
            notes: String::new(),
        }
    }

    #[test]
    fn upsert_by_id_is_case_insensitive_and_sorted() {
        let mut r = Roadmap::default();
        assert!(r.is_empty());
        r.upsert(milestone("0.3.29", "declared"));
        r.upsert(milestone("0.3.28", "current"));
        r.upsert(milestone("0.3.29", "declared again"));
        assert_eq!(r.milestones.len(), 2, "same id replaces, not duplicates");
        assert!(r.find("0.3.29").unwrap().state == "declared again");
    }

    #[test]
    fn find_is_trimmed_and_case_insensitive() {
        let mut r = Roadmap::default();
        r.upsert(milestone("Phase-H", "declared"));
        assert!(r.find("  phase-h  ").is_some());
        assert!(r.find("phase-g").is_none());
    }

    #[test]
    fn reached_means_done_and_answers_reviews() {
        let mut r = Roadmap::default();
        r.upsert(milestone("0.3.27", "done"));
        r.upsert(milestone("0.3.28", "current"));
        r.upsert(milestone("0.4.0", "declared"));
        assert!(r.reached("0.3.27"));
        assert!(r.reached("0.3.27 "), "review specs carry whitespace");
        assert!(!r.reached("0.3.28"), "only done counts");
        assert!(!r.reached("0.4.0"));
        assert!(!r.reached("never-declared"));
    }

    #[test]
    fn current_surfaces_at_most_the_single_current() {
        let mut r = Roadmap::default();
        assert!(r.current().is_none(), "empty roadmap, no current");
        r.upsert(milestone("0.3.28", "current"));
        assert_eq!(r.current().unwrap().id, "0.3.28");
    }

    #[test]
    fn coherence_names_duplicates_unknown_states_and_multi_currents() {
        let mut r = Roadmap::default();
        assert!(r.coherence().is_empty(), "empty is coherent");
        r.upsert(milestone("0.3.28", "current"));
        assert!(r.coherence().is_empty());
        // Direct pushes model hand-edited data: upsert alone can never
        // duplicate an id (it replaces), so coherence covers the file
        // being edited out-of-band.
        r.milestones.push(milestone("0.3.28", "done"));
        r.milestones.push(milestone("0.3.29", "almost"));
        r.milestones.push(milestone("0.3.30", "current"));
        let problems = r.coherence();
        assert_eq!(problems.len(), 3, "{problems:#?}");
        assert!(problems.iter().any(|p| p.contains("duplicate milestone id `0.3.28`")));
        assert!(problems.iter().any(|p| p.contains("unrecognized state `almost`")));
        assert!(problems.iter().any(|p| p.contains("2 milestones marked `current`")));
        r.remove("0.3.29");
        assert_eq!(r.coherence().len(), 2, "{:#?}", r.coherence());
    }

    #[test]
    fn remove_is_case_insensitive_and_honest() {
        let mut r = Roadmap::default();
        r.upsert(milestone("phase-h", "declared"));
        assert!(r.remove("PHASE-H"));
        assert!(r.is_empty());
        assert!(!r.remove("phase-h"));
    }
}
