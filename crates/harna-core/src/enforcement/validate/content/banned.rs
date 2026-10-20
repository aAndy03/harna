//! `banned-api` (0.3.45, deno study B3): paths a scope must not use, as
//! rule data — `values` lists them (`std::fs::read`, `std::process::exit`),
//! `rationale` says why. deno keeps the same lists in 61 hand-written
//! `clippy.toml` files plus a script checking each holds the required
//! entries; here the rule is the single source, this check enforces it
//! textually, and `harna deny --clippy` writes it into each crate's
//! `clippy.toml` so the compiler enforces it too.
//!
//! A value may name its replacement, `path => replacement` (0.3.48, R1):
//! the violation says where to go and `harna deny` writes clippy's
//! `replacement` field.
//!
//! A banned path also matches through its module alias: `use std::fs;`
//! then `fs::read(…)` is `std::fs::read`.

use super::{Severity, Violation};
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::path_within;
use crate::search::digest::live::live_code;

pub fn check(rules: &Rules, env: Option<&str>, file: &str, src: &str, out: &mut Vec<Violation>) {
    let bound: Vec<_> = rules
        .rules
        .iter()
        .filter(|r| {
            !r.is_deprecated()
                && r.matches_env(env)
                && r.check.as_deref() == Some("banned-api")
                && (r.scope.is_empty() || r.scope.iter().any(|s| path_within(file, s)))
        })
        .collect();
    if bound.is_empty() {
        return;
    }
    let live = live_code(src);
    // The file that implements the banned capability says so the way clippy
    // reads it: a reasoned file-level allow of disallowed_methods (deno's
    // ext/fs/std_fs.rs: "file system implementation"). harna honors the same
    // exemption it writes into clippy.toml.
    if live.iter().any(|(_, l)| {
        l.starts_with("#![allow(") && l.contains("disallowed_methods") && l.contains("reason")
    }) {
        return;
    }
    for rule in bound {
        for value in &rule.values {
            let (banned, replacement) = crate::schema::model::rules::value_and_replacement(value);
            let alias = alias_of(banned, &live);
            for (n, line) in &live {
                let hit = exact(line, banned) || alias.as_deref().is_some_and(|a| exact(line, a));
                if hit && !line.starts_with("use ") {
                    out.push(Violation {
                        check: "banned-api".into(),
                        file: file.into(),
                        line: *n,
                        severity: crate::enforcement::checks::severity_of("banned-api"),
                        message: format!("`{banned}` is banned here (rule `{}`)", rule.id),
                        hint: match replacement {
                            Some(to) => format!("use `{to}` instead — {}", rule.rationale),
                            None => format!(
                                "{} — remove it (the rule names no replacement)",
                                rule.rationale
                            ),
                        },
                    });
                }
            }
        }
    }
}

/// The banned path as a whole path: not the tail of a longer one
/// (`my_std::fs::read`), not the head of one (`fs::read_to_string`).
fn exact(line: &str, path: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices(path).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + path.len()..].chars().next();
        before.is_none_or(|c| !ident(c) && c != ':') && after.is_none_or(|c| !ident(c))
    })
}

/// Crates whose clippy.toml lacks a ban the rules require (Info): the
/// textual check holds meanwhile; `harna deny` writes the bans.
pub fn check_drift(root: &std::path::Path, rules: &Rules, out: &mut Vec<Violation>) {
    for (dir, missing) in crate::generation::views::clippy::drift(root, rules) {
        out.push(Violation {
            check: "banned-api".into(),
            file: format!("{dir}/clippy.toml"),
            line: 0,
            severity: Severity::Info,
            message: format!(
                "clippy.toml lacks {} banned path(s): {}",
                missing.len(),
                missing.join(", ")
            ),
            hint: "`harna deny` writes them, so the compiler enforces the ban too".into(),
        });
    }
}

/// `std::fs::read` reached as `fs::read` after `use std::fs;`.
fn alias_of(banned: &str, live: &[(usize, &str)]) -> Option<String> {
    let (module, item) = banned.rsplit_once("::")?;
    let (_, leaf) = module.rsplit_once("::")?;
    let imported = live.iter().any(|(_, l)| {
        l.starts_with("use ") && l.trim_end_matches(';').trim_start_matches("use ").trim() == module
    });
    imported.then(|| format!("{leaf}::{item}"))
}

#[cfg(test)]
mod tests;
