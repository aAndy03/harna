//! Component `rule-meaning` (0.4.8, codex/zed/ruff study R6): rules
//! attached to what code is ABOUT, not only where it sits.
//!
//! zed's lint applies to functions taking `&App` or `&mut Window`, wherever
//! they live; "money is never a float" applies to every module handling
//! `Money`. A rule's `scope_signature` is a pattern over declarations (the
//! map's signatures and inventory, plus the live declarations of files the
//! map has not seen yet): the rule applies to every file that declares a
//! match, intersected with its path `scope` when it has one. Validate
//! expands it into concrete files once, so every check honors it without
//! knowing about it.

use crate::schema::model::rules::{Rule, Rules};
use crate::search::digest::Digest;

/// What can never match a real path: a meaning-scoped rule with no
/// matching file (or no map) applies nowhere, not everywhere.
pub const NOWHERE: &str = "\u{0}nowhere";

/// Checks that judge a folder or a crate, not a file: a meaning scope (a
/// set of files) can never bind them, so the pairing is refused (0.4.9).
const DIR_LEVEL: &[&str] = &[
    "crate-conformity",
    "folder-size",
    "name-cluster",
    "folder-entries",
    "referenced-by",
    "generated",
];

/// Why a rule's `scope_signature` cannot work, if it cannot: shared by
/// add-rule (CLI and MCP), doctor and validate so all three agree.
pub fn problem(rule: &Rule) -> Option<String> {
    let pattern = rule.scope_signature.as_deref()?;
    if let Err(e) = regex::Regex::new(pattern) {
        let first = e.to_string();
        let first = first.lines().last().unwrap_or("").trim().to_string();
        return Some(format!(
            "scope_signature `{pattern}` is not a valid pattern ({first})"
        ));
    }
    if let Some(check) = rule.check.as_deref().filter(|c| DIR_LEVEL.contains(c)) {
        return Some(format!(
            "check `{check}` judges folders or crates, which a meaning scope (a set of files) never binds — use a path scope"
        ));
    }
    None
}

/// What resolving a meaning scope found wrong: broken rules are data
/// errors; a missing map only degrades.
pub struct Problems {
    pub broken: Vec<(String, String)>,
    pub degraded: Vec<(String, String)>,
}

/// Rewrite every meaning-scoped rule's `scope` into the files it means.
/// `live` holds (path, source) of files the map may not know yet — new or
/// changed in this burst — matched on their declaration lines.
pub fn expand(rules: &mut Rules, digest: Option<&Digest>, live: &[(String, String)]) -> Problems {
    let mut p = Problems {
        broken: Vec::new(),
        degraded: Vec::new(),
    };
    for r in rules.rules.iter_mut() {
        let Some(pattern) = r.scope_signature.clone() else {
            continue;
        };
        if let Some(why) = problem(r) {
            p.broken.push((r.id.clone(), why));
            r.scope = vec![NOWHERE.into()];
            continue;
        }
        let Ok(re) = regex::Regex::new(&pattern) else {
            continue; // unreachable: problem() checked it
        };
        if digest.is_none() && live.is_empty() {
            p.degraded.push((
                r.id.clone(),
                "scoped by meaning, but there is no map — run `harna scan`".into(),
            ));
            r.scope = vec![NOWHERE.into()];
            continue;
        }
        let paths = r.scope.clone();
        let within = |path: &str| {
            paths.is_empty()
                || paths
                    .iter()
                    .any(|s| crate::schema::model::scope::path_within(path, s))
        };
        let mut files: Vec<String> = Vec::new();
        let live_paths: std::collections::HashSet<&str> =
            live.iter().map(|(f, _)| f.as_str()).collect();
        if let Some(d) = digest {
            files.extend(
                d.modules
                    .iter()
                    // a live file is judged on its source, not a stale map
                    .filter(|m| !live_paths.contains(m.path.as_str()) && within(&m.path))
                    .filter(|m| {
                        m.signatures.iter().any(|s| re.is_match(s))
                            || m.inventory
                                .iter()
                                .any(|i| re.is_match(&i.text) || re.is_match(&i.name))
                    })
                    .map(|m| m.path.clone()),
            );
        }
        files.extend(
            live.iter()
                .filter(|(f, src)| within(f) && declares(src, &re))
                .map(|(f, _)| f.clone()),
        );
        files.sort();
        files.dedup();
        r.scope = if files.is_empty() {
            vec![NOWHERE.into()]
        } else {
            files
        };
    }
    p
}

/// Does live source declare a match? Declaration lines only (items, fields
/// and parameters — what the map's signatures hold), never comments.
fn declares(src: &str, re: &regex::Regex) -> bool {
    let decl = regex::Regex::new(
        r"^(pub(\([^)]*\))?\s+)?((async|const|unsafe|extern)\s+)*(fn|struct|enum|trait|type|impl|const|static|mod|use)\b|^(pub(\([^)]*\))?\s+)?[a-z_][a-z0-9_]*\s*:",
    )
    .expect("static pattern");
    src.lines()
        .map(str::trim_start)
        .filter(|l| !l.starts_with("//"))
        .any(|l| decl.is_match(l) && re.is_match(l))
}

#[cfg(test)]
mod tests;
