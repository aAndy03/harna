//! `folder-entries` (E1): a folder holds only the entries its rule lists.
//! deno's `ensureNoNewTopLevelEntries`: "When adding anything to this list
//! it must be discussed!" — a new entry is a decision, not drift. The rule's
//! scope is the folder (empty or `.` = the repo root), `values` the names.

use std::path::Path;

use super::{bound, finding, Violation};
use crate::schema::model::rules::Rules;

pub fn check(root: &Path, rules: &Rules, env: Option<&str>, out: &mut Vec<Violation>) {
    for rule in bound(rules, env, "folder-entries") {
        let folders: Vec<String> = if rule.scope.is_empty() {
            vec![String::new()]
        } else {
            rule.scope
                .iter()
                .map(|s| crate::schema::model::scope::normalize_scope(s))
                .collect()
        };
        for folder in folders {
            let dir = if folder.is_empty() {
                root.to_path_buf()
            } else {
                root.join(&folder)
            };
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !matches!(n.as_str(), ".git" | ".harna" | "target"))
                .collect();
            names.sort();
            for name in names.iter().filter(|n| !rule.values.contains(n)) {
                let shown = if folder.is_empty() {
                    name.clone()
                } else {
                    format!("{folder}/{name}")
                };
                out.push(finding(
                    "folder-entries",
                    &shown,
                    0,
                    format!(
                        "`{name}` is not an allowed entry of {} (rule `{}`)",
                        if folder.is_empty() {
                            "the repo root"
                        } else {
                            &folder
                        },
                        rule.id
                    ),
                    format!(
                        "{} — adding an entry is a decision: list it in the rule's values",
                        rule.rationale
                    ),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::rules::Rule;

    #[test]
    fn unlisted_entries_are_named() {
        let root = std::env::temp_dir().join(format!(
            "harna-entries-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["cli", "ext", "scratch"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        let mut rules = Rules::default();
        rules.rules.push(Rule {
            id: "top-level".into(),
            text: "t".into(),
            rationale: "must be discussed".into(),
            origin: "o".into(),
            check: Some("folder-entries".into()),
            values: vec!["cli".into(), "ext".into(), "Cargo.toml".into()],
            ..Default::default()
        });
        let mut out = Vec::new();
        check(&root, &rules, None, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].file, "scratch");
        assert!(out[0]
            .message
            .contains("not an allowed entry of the repo root"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
