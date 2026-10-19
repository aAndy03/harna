//! `referenced-by` (E3): every file in the rule's scope is referenced by
//! name from some file under its `values` folders. deno's
//! `ensureNoUnusedOutFiles`: every `tests/specs/**/*.out` must appear in
//! some `__test__.jsonc` — orphan fixtures are dead weight nobody notices.
//! The rule's `pattern`, when set, filters which file names must be
//! referenced (deno: `\.out$`); the reference is the file name appearing in
//! a referencing file's text.

use std::path::Path;

use super::{bound, files_in, finding, Violation};
use crate::schema::model::rules::Rules;

pub fn check(root: &Path, rules: &Rules, env: Option<&str>, out: &mut Vec<Violation>) {
    for rule in bound(rules, env, "referenced-by") {
        if rule.values.is_empty() {
            out.push(finding(
                "referenced-by",
                ".harna/rules.toml",
                0,
                format!("rule `{}` names no referencing folders (`values`)", rule.id),
                "list the folders whose files must reference the scope".into(),
            ));
            continue;
        }
        let filter = rule
            .pattern
            .as_deref()
            .and_then(|p| regex::Regex::new(p).ok());
        // Every file-name-shaped token of the referencing files, collected
        // once (0.3.46: a substring search per fixture over the joined
        // text took 57s on deno's 2,816 .out files).
        let mut tokens: std::collections::HashSet<String> = std::collections::HashSet::new();
        for f in rule.values.iter().flat_map(|v| files_in(root, v)) {
            let name = f.rsplit('/').next().unwrap_or(&f);
            if filter.as_ref().is_some_and(|re| re.is_match(name)) {
                continue; // a file never counts as its own reference
            }
            let Ok(text) = std::fs::read_to_string(root.join(&f)) else {
                continue;
            };
            for tok in
                text.split(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '/')))
            {
                if tok.is_empty() {
                    continue;
                }
                // `run/main.out` references `main.out` too
                tokens.insert(tok.rsplit('/').next().unwrap_or(tok).to_string());
            }
        }
        for scope in &rule.scope {
            for file in files_in(root, scope) {
                let name = file.rsplit('/').next().unwrap_or(&file);
                if filter.as_ref().is_some_and(|re| !re.is_match(name)) {
                    continue;
                }
                if !tokens.contains(name) {
                    out.push(finding(
                        "referenced-by",
                        &file,
                        0,
                        format!(
                            "nothing under {} references `{name}` (rule `{}`)",
                            rule.values.join(", "),
                            rule.id
                        ),
                        "reference it or delete it — an orphan fixture is dead weight".into(),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::rules::Rule;

    #[test]
    fn orphan_files_are_named() {
        let root = std::env::temp_dir().join(format!(
            "harna-referenced-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("tests/specs/run")).unwrap();
        std::fs::write(
            root.join("tests/specs/run/__test__.jsonc"),
            r#"{"args": "run main.ts", "output": "main.out"}"#,
        )
        .unwrap();
        std::fs::write(root.join("tests/specs/run/main.ts"), "console.log(1)").unwrap();
        std::fs::write(root.join("tests/specs/run/main.out"), "1").unwrap();
        std::fs::write(root.join("tests/specs/run/stale.out"), "old").unwrap();
        let mut rules = Rules::default();
        rules.rules.push(Rule {
            id: "no-orphan-out".into(),
            text: "t".into(),
            rationale: "r".into(),
            origin: "o".into(),
            check: Some("referenced-by".into()),
            scope: vec!["tests/specs".into()],
            pattern: Some(r"\.out$".into()),
            values: vec!["tests/specs".into()],
            ..Default::default()
        });
        let mut out = Vec::new();
        check(&root, &rules, None, &mut out);
        let files: Vec<&str> = out.iter().map(|v| v.file.as_str()).collect();
        assert_eq!(files, vec!["tests/specs/run/stale.out"], "{out:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
