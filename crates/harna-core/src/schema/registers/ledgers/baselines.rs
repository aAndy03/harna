//! Component `baselines-data` (0.3.44): the RATCHET ledger's data.
//!
//! A baseline freezes today's error count for one check in one file: the
//! gate lets up to that many through (as receipts, never silently), fails
//! the moment the count grows, and says when it can tighten. That is how
//! an error-severity check is adopted on a codebase that already breaks
//! it (deno ratchets `Deno.*` uses in polyfills the same way, by hand,
//! with `EXPECTED_VIOLATIONS` counts that may only decrease). Unlike an
//! acknowledgment — an Info accepted for good — a baseline is debt with a
//! direction: down.

use serde::{Deserialize, Serialize};

fn current() -> u32 {
    crate::SCHEMA_VERSION
}

/// Top-level ratchet ledger (`.harna/baselines.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Baselines {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<Baseline>,
}

/// Allowed error count for (check, file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub check: String,
    pub file: String,
    pub count: usize,
    /// When it was recorded or last tightened (ISO date).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recorded: String,
}

impl Baselines {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn allowed(&self, check: &str, file: &str) -> Option<usize> {
        self.entries
            .iter()
            .find(|e| e.check == check && e.file == file)
            .map(|e| e.count)
    }

    /// Set (check, file) to `count`; zero removes the entry — a paid-off
    /// debt leaves no line behind.
    pub fn set(&mut self, check: &str, file: &str, count: usize, recorded: &str) {
        self.entries
            .retain(|e| !(e.check == check && e.file == file));
        if count > 0 {
            self.entries.push(Baseline {
                check: check.into(),
                file: file.into(),
                count,
                recorded: recorded.into(),
            });
        }
        self.entries
            .sort_by(|a, b| a.check.cmp(&b.check).then(a.file.cmp(&b.file)));
    }

    pub fn total(&self) -> usize {
        self.entries.iter().map(|e| e.count).sum()
    }
}
