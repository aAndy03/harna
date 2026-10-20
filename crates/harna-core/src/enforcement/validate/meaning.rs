//! Component `rule-meaning` (0.4.8, codex/zed/ruff study R6): rules
//! attached to what code is ABOUT, not only where it sits.
//!
//! zed's lint applies to functions taking `&App` or `&mut Window`, wherever
//! they live; "money is never a float" applies to every module handling
//! `Money`. A rule's `scope_signature` is a pattern over declarations (the
//! map's signatures and inventory): the rule applies to every file that
//! declares a match, intersected with its path `scope` when it has one.
//! Validate expands it into concrete files once, so every check honors it
//! without knowing about it.

use crate::schema::model::rules::Rules;
use crate::search::digest::Digest;

/// What can never match a real path: a meaning-scoped rule with no
/// matching file (or no map) applies nowhere, not everywhere.
const NOWHERE: &str = "\u{0}nowhere";

/// Rewrite every meaning-scoped rule's `scope` into the files it means.
/// Returns (rule id, reason) for rules that could not be resolved.
pub fn expand(rules: &mut Rules, digest: Option<&Digest>) -> Vec<(String, String)> {
    let mut problems = Vec::new();
    for r in rules.rules.iter_mut() {
        let Some(pattern) = r.scope_signature.clone() else {
            continue;
        };
        let re = match regex::Regex::new(&pattern) {
            Ok(re) => re,
            Err(e) => {
                problems.push((
                    r.id.clone(),
                    format!("scope_signature is not a valid pattern: {e}"),
                ));
                r.scope = vec![NOWHERE.into()];
                continue;
            }
        };
        let Some(d) = digest else {
            problems.push((
                r.id.clone(),
                "scoped by meaning, but there is no map — run `harna scan`".into(),
            ));
            r.scope = vec![NOWHERE.into()];
            continue;
        };
        let paths = r.scope.clone();
        let files: Vec<String> = d
            .modules
            .iter()
            .filter(|m| {
                paths.is_empty()
                    || paths
                        .iter()
                        .any(|s| crate::schema::model::scope::path_within(&m.path, s))
            })
            .filter(|m| {
                m.signatures.iter().any(|s| re.is_match(s))
                    || m.inventory
                        .iter()
                        .any(|i| re.is_match(&i.text) || re.is_match(&i.name))
            })
            .map(|m| m.path.clone())
            .collect();
        r.scope = if files.is_empty() {
            vec![NOWHERE.into()]
        } else {
            files
        };
    }
    problems
}

#[cfg(test)]
mod tests;
