//! Component `commands-data` (0.3.46, deno study F3): the repo's own
//! commands as data — what to run, what it proves, where it runs.
//!
//! deno's CLAUDE.md says "Do NOT run these directly with
//! `./target/debug/deno test`" and its task runner defines `test-unit`,
//! `test-spec`, …; CONTRIBUTING.md meanwhile documents `./x test`, which
//! does not exist. A registry gives agents the right command per scope in
//! onboarding, and gives `doc-claims` something to check prose against.

use serde::{Deserialize, Serialize};

fn current() -> u32 {
    crate::SCHEMA_BASELINE
}

/// `.harna/commands.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Commands {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<Command>,
}

/// One command the repo runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Command {
    /// Stable id, e.g. "test-spec".
    pub name: String,
    /// The exact invocation, e.g. "./x test-spec".
    pub run: String,
    /// What a pass proves, e.g. "spec tests under tests/specs".
    pub proves: String,
    /// "local" | "ci" | "both" (default both).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub runs: String,
    /// Paths it covers; empty = the whole repo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
}

impl Commands {
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Add or replace by name.
    pub fn upsert(&mut self, c: Command) {
        self.commands.retain(|x| x.name != c.name);
        self.commands.push(c);
        self.commands.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.commands.len();
        self.commands.retain(|x| x.name != name);
        before != self.commands.len()
    }

    /// Commands covering `path` (scope-less ones cover everything).
    pub fn for_path(&self, path: &str) -> Vec<&Command> {
        self.commands
            .iter()
            .filter(|c| {
                c.scope.is_empty()
                    || c.scope
                        .iter()
                        .any(|s| crate::schema::model::scope::path_within(path, s))
            })
            .collect()
    }

    /// The runners (first word of `run`) and the second words registered
    /// under each — what `doc-claims` checks cited invocations against.
    pub fn runners(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for c in &self.commands {
            let mut words = c.run.split_whitespace();
            let (Some(runner), sub) = (words.next(), words.next()) else {
                continue;
            };
            let entry = match out.iter().position(|(r, _)| r == runner) {
                Some(i) => &mut out[i].1,
                None => {
                    out.push((runner.to_string(), Vec::new()));
                    &mut out.last_mut().expect("just pushed").1
                }
            };
            if let Some(s) = sub {
                if !entry.iter().any(|x| x == s) {
                    entry.push(s.to_string());
                }
            }
        }
        out
    }
}
