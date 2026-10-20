//! `crate-conformity` (0.3.48, R4): what every crate in scope must carry,
//! as rule data. codex mirrors each crate's Cargo.toml in a `BUILD.bazel`
//! and a script keeps their lints in sync; zed's `package_conformity.rs`
//! insists every crate's Cargo.toml says `lints.workspace = true`. Here a
//! rule's `values` list the requirements, each named per crate:
//!
//! - `file:BUILD.bazel` — a companion file in the crate dir;
//! - `toml:lints.workspace` — a Cargo.toml key is present;
//! - `toml:lints.workspace = true` — and holds this TOML value;
//! - a leading `!` inverts any of them (0.4.2): `!toml:dependencies.gpui`
//!   — the crate must NOT depend on gpui directly.

use std::path::Path;

use super::Violation;
use crate::enforcement::validate::hygiene::{bound, finding};
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::path_within;
use crate::search::digest::crates::CrateIndex;

pub fn check(root: &Path, rules: &Rules, env: Option<&str>, out: &mut Vec<Violation>) {
    let rules_here = bound(rules, env, "crate-conformity");
    if rules_here.is_empty() {
        return;
    }
    let index = CrateIndex::load(root);
    for rule in rules_here {
        for c in index
            .crates
            .iter()
            .filter(|c| rule.scope.is_empty() || rule.scope.iter().any(|s| path_within(&c.dir, s)))
        {
            let base = if c.dir == "." {
                root.to_path_buf()
            } else {
                root.join(&c.dir)
            };
            let cargo: Option<toml::Value> = std::fs::read_to_string(base.join("Cargo.toml"))
                .ok()
                .and_then(|s| toml::from_str(&s).ok());
            let missing: Vec<&str> = rule
                .values
                .iter()
                .map(String::as_str)
                .filter(|v| !holds(v, &base, cargo.as_ref()))
                .collect();
            if missing.is_empty() {
                continue;
            }
            let manifest = if c.dir == "." {
                "Cargo.toml".to_string()
            } else {
                format!("{}/Cargo.toml", c.dir)
            };
            out.push(finding(
                "crate-conformity",
                &manifest,
                0,
                format!(
                    "crate `{}` lacks {} (rule `{}`)",
                    c.package,
                    missing.join(", "),
                    rule.id
                ),
                format!("every crate in scope carries these — {}", rule.rationale),
            ));
        }
    }
}

/// Whether one requirement holds for the crate at `base`.
fn holds(requirement: &str, base: &Path, cargo: Option<&toml::Value>) -> bool {
    if let Some(inner) = requirement.strip_prefix('!') {
        return !holds_positive(inner.trim(), base, cargo);
    }
    holds_positive(requirement, base, cargo)
}

fn holds_positive(requirement: &str, base: &Path, cargo: Option<&toml::Value>) -> bool {
    if let Some(file) = requirement.strip_prefix("file:") {
        return base.join(file.trim()).exists();
    }
    let Some(spec) = requirement.strip_prefix("toml:") else {
        return true; // not a requirement this check knows: the doctor names it
    };
    let (key, want) = match spec.split_once('=') {
        Some((k, v)) => (k.trim(), Some(v.trim())),
        None => (spec.trim(), None),
    };
    let mut at = cargo;
    for part in key.split('.') {
        at = at.and_then(|v| v.get(part));
    }
    match (at, want) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(v), Some(w)) => toml::from_str::<toml::Table>(&format!("x = {w}"))
            .ok()
            .and_then(|t| t.get("x").cloned())
            .is_some_and(|w| &w == v),
    }
}

#[cfg(test)]
mod tests;
