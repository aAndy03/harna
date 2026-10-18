//! Component `terms-data` (0.3.26): the GLOSSARY's data structs live in
//! the memory layer — same discipline as `parity-data` and
//! `deferred-data`: the store persists the document, and the pure data
//! logic (alias collisions) is testable without a filesystem.
//!
//! A term maps the agent vocabulary to harna's: agents bring their own
//! words (a model says "service" where harna says "engine"), and the
//! glossary is the fixed point they can trust — harna's vocabulary is
//! harvested, never renamed to match a model. The document is data;
//! `onboarding` renders it compressed, `doctor` checks it coherent, and
//! the MCP surface exposes it read-only.

use serde::{Deserialize, Serialize};

/// Version of the glossary document (`.harna/terms.toml`).
fn current() -> u32 {
    crate::SCHEMA_VERSION
}

/// Top-level glossary document (`.harna/terms.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Terms {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terms: Vec<Term>,
}

/// One vocabulary entry: harna's `word` (the fixed point), the agent
/// aliases it absorbs (harvested from actual agent sessions, not
/// invented), and what the word MEANS in harna's terms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Term {
    /// harna's canonical word, e.g. "engine".
    pub word: String,
    /// Agent vocabulary that maps to this word, e.g. ["service", "module-group"].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// The meaning — as harna defines it, not as the aliasing agent did.
    pub means: String,
}

impl Terms {
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Words and aliases colliding across entries: a vocabulary where one
    /// spelling means two things is worse than no glossary at all (the
    /// doctor names these; case-insensitive, so "Engine" and "engine"
    /// collide too).
    pub fn alias_collisions(&self) -> Vec<(String, String)> {
        let mut spellings: Vec<(String, String)> = Vec::new(); // (lowercased spelling, owner word)
        for t in &self.terms {
            spellings.push((t.word.to_lowercase(), t.word.clone()));
            for a in &t.aliases {
                spellings.push((a.to_lowercase(), t.word.clone()));
            }
        }
        let mut out: Vec<(String, String)> = Vec::new();
        for i in 0..spellings.len() {
            for j in (i + 1)..spellings.len() {
                let (a, a_owner) = &spellings[i];
                let (b, b_owner) = &spellings[j];
                if a == b && a_owner != b_owner {
                    // One spelling owning itself via its own alias chain is
                    // still a collision only across different owners.
                    let pair = (format!("{a_owner}/{b_owner}"), format!("`{b}`"));
                    if !out.contains(&pair) {
                        out.push(pair);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(word: &str, aliases: &[&str]) -> Term {
        Term {
            word: word.into(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            means: "test meaning".into(),
        }
    }

    #[test]
    fn a_single_term_never_collides_with_itself() {
        let mut t = Terms::default();
        t.terms.push(term("engine", &["service"]));
        // "engine" owner owns both its word and its alias: same owner.
        assert!(t.alias_collisions().is_empty());

        t.terms.push(term("component", &["module"]));
        assert!(t.alias_collisions().is_empty());
    }

    #[test]
    fn cross_owner_collisions_are_named_once_per_spelling() {
        let mut t = Terms::default();
        t.terms.push(term("engine", &["module-group"]));
        t.terms.push(term("container", &["module-group", "Module-Group"]));
        let c = t.alias_collisions();
        assert_eq!(c.len(), 1, "case-insensitive duplicates collapse: {c:?}");
        assert!(c[0].0.contains("engine"), "owners named: {}", c[0].0);
        assert!(c[0].0.contains("container"));
    }

    #[test]
    fn a_word_stealing_another_words_alias_collides() {
        let mut t = Terms::default();
        t.terms.push(term("parity", &["ripple"]));
        t.terms.push(term("ripple", &[]));
        assert_eq!(t.alias_collisions().len(), 1, "`ripple` is a term AND parity's alias");
    }

    #[test]
    fn an_empty_glossary_is_the_adopted_empty_state() {
        let t = Terms::default();
        assert!(t.is_empty());
        assert!(t.alias_collisions().is_empty());
    }
}
