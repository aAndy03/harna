//! `sync-group` (0.3.45, deno study D1): lists that must stay identical
//! across files, as data. deno registers extensions in four files with the
//! comment "ordering is important here, keep it in sync with…" and no test
//! enforcing it. A rule names the files (`values`) and the extractor
//! (`pattern`, capture group 1); every file must yield the same ordered
//! list as the first.

use std::path::Path;

use regex::Regex;

use super::Violation;
use crate::schema::model::rules::Rules;
use crate::search::digest::live::live_code;

pub fn check(root: &Path, rules: &Rules, env: Option<&str>, out: &mut Vec<Violation>) {
    for rule in rules.rules.iter().filter(|r| {
        !r.is_deprecated() && r.matches_env(env) && r.check.as_deref() == Some("sync-group")
    }) {
        let violation = |file: &str, message: String| Violation {
            check: "sync-group".into(),
            file: file.into(),
            line: 0,
            severity: crate::enforcement::checks::severity_of("sync-group"),
            message: format!("{message} (rule `{}`)", rule.id),
            hint: "keep every site of the group identical and in the same order".into(),
        };
        let Some(re) = rule
            .pattern
            .as_deref()
            .and_then(|p| Regex::new(p).ok())
            .filter(|re| re.captures_len() > 1)
        else {
            out.push(violation(
                ".harna/rules.toml",
                "sync-group needs a `pattern` with one capture group".into(),
            ));
            continue;
        };
        let lists: Vec<(&String, Option<Vec<String>>)> = rule
            .values
            .iter()
            .map(|f| (f, extract(&root.join(f), &re)))
            .collect();
        let Some((first, Some(reference))) = lists.first().map(|(f, l)| (*f, l.clone())) else {
            if let Some((f, _)) = lists.first() {
                out.push(violation(f, "the group's first site does not exist".into()));
            }
            continue;
        };
        for (file, list) in lists.iter().skip(1) {
            match list {
                None => out.push(violation(file, "this group site does not exist".into())),
                Some(list) if list != &reference => {
                    out.push(violation(file, differ(list, &reference, first)))
                }
                Some(_) => {}
            }
        }
    }
}

fn extract(path: &Path, re: &Regex) -> Option<Vec<String>> {
    let src = std::fs::read_to_string(path).ok()?;
    Some(
        live_code(&src)
            .into_iter()
            .flat_map(|(_, line)| {
                re.captures_iter(line)
                    .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                    .collect::<Vec<_>>()
            })
            .collect(),
    )
}

fn differ(list: &[String], reference: &[String], first: &str) -> String {
    let missing: Vec<&str> = reference
        .iter()
        .filter(|x| !list.contains(x))
        .map(String::as_str)
        .collect();
    let extra: Vec<&str> = list
        .iter()
        .filter(|x| !reference.contains(x))
        .map(String::as_str)
        .collect();
    if missing.is_empty() && extra.is_empty() {
        let at = list
            .iter()
            .zip(reference)
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        return format!(
            "order differs from `{first}` at position {} (`{}` where `{}` stands there)",
            at + 1,
            list[at],
            reference[at]
        );
    }
    let mut parts = Vec::new();
    if !missing.is_empty() {
        parts.push(format!("missing {}", missing.join(", ")));
    }
    if !extra.is_empty() {
        parts.push(format!("extra {}", extra.join(", ")));
    }
    format!("out of sync with `{first}`: {}", parts.join("; "))
}

#[cfg(test)]
mod tests;
