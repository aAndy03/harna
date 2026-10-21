//! Check `append-only` (1.1.3): what was committed under the rule's paths
//! stays as it was. A migration that already ran in production, an event
//! schema old logs were written with: editing one rewrites history the
//! deployed system already lives in. New files are fine; a modified,
//! deleted or renamed committed file is the finding.
//!
//! The rule names the paths in `values` as globs (`db/migrations/**`,
//! `migrations/*.sql`).

use super::super::Violation;
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::glob_match;

pub fn check(
    rules: &Rules,
    env: Option<&str>,
    base: &str,
    changed: &[(char, String)],
    out: &mut Vec<Violation>,
) {
    for rule in rules.rules.iter().filter(|r| {
        !r.is_deprecated() && r.matches_env(env) && r.check.as_deref() == Some("append-only")
    }) {
        if rule.values.is_empty() {
            out.push(Violation {
                check: "harna-data".into(),
                file: ".harna/rules.toml".into(),
                line: 0,
                severity: crate::enforcement::violation::Severity::Error,
                message: format!(
                    "rule `{}` binds append-only but names no paths — it checks nothing",
                    rule.id
                ),
                hint: "add the paths: harna add-rule --replace ... --value 'db/migrations/**'"
                    .into(),
            });
            continue;
        }
        for (status, path) in changed {
            if *status == 'A' {
                continue; // adding is what append-only allows
            }
            if !rule.values.iter().any(|g| glob_match(g, path)) {
                continue;
            }
            let what = match status {
                'D' => "deleted",
                'R' => "renamed",
                _ => "edited",
            };
            out.push(Violation {
                check: "append-only".into(),
                file: path.clone(),
                line: 0,
                severity: crate::enforcement::checks::severity_of("append-only"),
                message: format!(
                    "{what} since {base}, under append-only paths: committed history changes (rule `{}`)",
                    rule.id
                ),
                hint: format!(
                    "add a new file that makes the change instead (a new migration), and restore this one; {}",
                    rule.rationale
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::rules::Rule;

    #[test]
    fn committed_files_under_the_paths_are_never_edited() {
        let rules = Rules {
            rules: vec![Rule {
                id: "migrations".into(),
                text: "t".into(),
                rationale: "they ran in production".into(),
                check: Some("append-only".into()),
                values: vec!["db/migrations/**".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let changed = vec![
            ('A', "db/migrations/003_add.sql".to_string()),
            ('M', "db/migrations/001_init.sql".to_string()),
            ('D', "db/migrations/002_x.sql".to_string()),
            ('M', "src/app.rs".to_string()),
        ];
        let mut out = Vec::new();
        check(&rules, None, "HEAD", &changed, &mut out);
        let files: Vec<&str> = out.iter().map(|v| v.file.as_str()).collect();
        assert_eq!(
            files,
            vec!["db/migrations/001_init.sql", "db/migrations/002_x.sql"]
        );
        assert!(out[1].message.starts_with("deleted"));
    }
}
