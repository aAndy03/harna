//! Amendment history (0.3.43, ledgerly finding 9: counts corrected from
//! 7/3 to 6/4 left no trace of the original). A leaf: the ledger root and
//! the resolve path both use it, it uses neither.

use serde::{Deserialize, Serialize};

/// One earlier verdict of a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Amendment {
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub when: String,
}

impl Amendment {
    /// A hand verdict (ack/noop) is worth keeping; open, origin and auto
    /// are machine states — nothing human is lost when they change.
    pub fn is_hand(&self) -> bool {
        matches!(self.status.as_str(), "ack" | "noop")
    }
}

/// How many earlier verdicts a target keeps (oldest drop first).
pub const HISTORY_CAP: usize = 8;

pub fn push_capped(history: &mut Vec<Amendment>, a: Amendment) {
    history.push(a);
    if history.len() > HISTORY_CAP {
        let drop = history.len() - HISTORY_CAP;
        history.drain(..drop);
    }
}

/// History for a re-opened target: the prior history, plus the prior hand
/// verdict when the new generation does not keep it.
pub fn carry(prior_history: &[Amendment], prior: Amendment, new_status: &str) -> Vec<Amendment> {
    let mut h = prior_history.to_vec();
    if prior.is_hand() && prior.status != new_status {
        push_capped(&mut h, prior);
    }
    h
}

/// The earlier verdicts, newest first, indented under their target.
pub fn render(history: &[Amendment]) -> String {
    history
        .iter()
        .rev()
        .map(|a| {
            let note = if a.note.is_empty() {
                String::new()
            } else {
                format!(": {}", a.note)
            };
            format!(
                "      earlier: {} by {} @ {}{note}\n",
                a.status,
                if a.by.is_empty() { "-" } else { &a.by },
                a.when.get(..10).unwrap_or(&a.when)
            )
        })
        .collect()
}
