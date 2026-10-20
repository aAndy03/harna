//! Component `qualities-data` (0.4.0): long-term intent as data.
//!
//! A quality states what the software must hold over its life, in the
//! project's own words: "1M users within two years", "p95 under 200 ms",
//! "works fully offline", "no data loss on crash", "GDPR export and
//! delete". Code answers today's request; qualities say what tomorrow
//! must still survive, so every session plans against them — not only the
//! first one. harna stores and shows them; it never decides them (rule
//! `tools-not-prescriptions`): the category is free text, the measure is
//! the project's own.

use serde::{Deserialize, Serialize};

use crate::schema::model::scope::path_within;

fn current() -> u32 {
    crate::SCHEMA_BASELINE
}

/// The qualities register (`.harna/qualities.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Qualities {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qualities: Vec<Quality>,
}

/// One quality the software must hold.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Quality {
    /// Stable id, e.g. "scale-users".
    pub id: String,
    /// What must hold, in plain words.
    pub statement: String,
    /// How it is measured or bounded, when it can be ("1M users",
    /// "p95 < 200ms", "RPO 0"). Free text: the project's own number.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub measure: String,
    /// A grouping word the project chooses (scale, latency, availability,
    /// durability, privacy, security, connectivity, …). Free text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    /// Where it applies; empty = the whole system.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
    /// Why it matters (the business or user reason).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
    /// When it was recorded (ISO date, filled by the CLI/MCP).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recorded: String,
    /// The rules that hold it (0.4.10, field test 049: `no-network` and
    /// `money-exact` stayed prose while a rule enforced each). Empty =
    /// prose only, a legitimate choice the doctor counts but never fails.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held_by: Vec<String>,
}

/// The first `held_by` id that names no rule, if any — the CLI and MCP
/// edges refuse it in the same words (0.4.10).
pub fn unknown_holder<'a>(
    held_by: &'a [String],
    rules: &crate::schema::model::rules::Rules,
) -> Option<&'a String> {
    held_by
        .iter()
        .find(|r| !rules.rules.iter().any(|x| &x.id == *r))
}

impl Quality {
    /// One line for briefings.
    pub fn line(&self) -> String {
        let mut out = format!("[{}] {}", self.id, self.statement);
        // the category is the project's grouping word (0.9.0: it was
        // written and never shown)
        if !self.category.is_empty() {
            out.push_str(&format!(" <{}>", self.category));
        }
        if !self.measure.is_empty() {
            out.push_str(&format!(" ({})", self.measure));
        }
        if !self.scope.is_empty() {
            out.push_str(&format!(" — scope: {}", self.scope.join(", ")));
        }
        if !self.held_by.is_empty() {
            out.push_str(&format!(" — held by: {}", self.held_by.join(", ")));
        }
        out
    }
}

impl Qualities {
    pub fn is_empty(&self) -> bool {
        self.qualities.is_empty()
    }

    /// The lowest schema that holds this register (see `SCHEMA_BASELINE`).
    pub fn required_schema(&self) -> u32 {
        if self.qualities.iter().any(|q| !q.held_by.is_empty()) {
            3 // held_by arrived after v2 shipped (0.5.9)
        } else {
            crate::SCHEMA_BASELINE
        }
    }

    /// Add or replace by id (case-insensitive: a re-statement refines it).
    /// A quality needs an id and a statement (0.4.9: both edges accepted
    /// empty ones and only the doctor noticed later).
    pub fn upsert(&mut self, q: Quality) -> anyhow::Result<()> {
        if q.id.trim().is_empty() {
            anyhow::bail!("a quality needs an id");
        }
        if q.statement.trim().is_empty() {
            anyhow::bail!("quality `{}` needs a statement — what must hold", q.id);
        }
        self.qualities.retain(|x| !x.id.eq_ignore_ascii_case(&q.id));
        self.qualities.push(q);
        self.qualities.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.qualities.len();
        self.qualities.retain(|q| !q.id.eq_ignore_ascii_case(id));
        before != self.qualities.len()
    }

    /// The qualities that bear on code under `paths`: system-wide ones and
    /// those whose scope covers (or sits inside) one of the paths. No
    /// paths = only the system-wide ones.
    pub fn in_scope(&self, paths: &[&str]) -> Vec<&Quality> {
        self.qualities
            .iter()
            .filter(|q| {
                q.scope.is_empty()
                    || q.scope
                        .iter()
                        .any(|s| paths.iter().any(|p| path_within(p, s) || path_within(s, p)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
