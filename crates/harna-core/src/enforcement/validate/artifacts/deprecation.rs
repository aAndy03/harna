//! `deprecation-window` (0.7.4): deprecated items have a removal date, in
//! versions, and harna counts it down.
//!
//! The code says what is deprecated and since when — `#[deprecated(since
//! = "1.4.0")]` in Rust, `@deprecated since 1.4.0` in a TS/JS doc comment.
//! A rule bound to this check gives the window in minor versions
//! (`--limit 2`); the project's version comes from its root `Cargo.toml`
//! (`[workspace.package]` or `[package]`) or `package.json`. Validate names
//! every item whose window has closed (remove it now), and `harna
//! deprecations` lists the countdown for all of them.

use std::path::Path;

use regex::Regex;

use super::Violation;
use crate::schema::model::rules::Rules;

#[derive(Debug, Clone, PartialEq)]
pub struct Deprecated {
    pub file: String,
    pub line: usize,
    pub name: String,
    pub since: String,
}

/// The project's version, if it declares one.
pub fn project_version(root: &Path) -> Option<String> {
    if let Ok(raw) = std::fs::read_to_string(root.join("Cargo.toml")) {
        if let Ok(v) = toml::from_str::<toml::Value>(&raw) {
            let from = |t: Option<&toml::Value>| {
                t.and_then(|p| p.get("version"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            };
            if let Some(ver) = from(v.get("workspace").and_then(|w| w.get("package")))
                .or_else(|| from(v.get("package")))
            {
                return Some(ver);
            }
        }
    }
    let raw = std::fs::read_to_string(root.join("package.json")).ok()?;
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()?
        .get("version")?
        .as_str()
        .map(str::to_string)
}

/// Every deprecated item the tree declares.
pub fn scan(root: &Path) -> Vec<Deprecated> {
    let since_re = Regex::new(r#"since\s*=\s*"([^"]+)""#).expect("static");
    let ts_doc = Regex::new(r"@deprecated\s+(?:since\s+)?v?(\d+\.\d+(?:\.\d+)?)").expect("static");
    let item = Regex::new(
        r"\b(?:fn|struct|enum|trait|type|const|static|mod|function|class|interface)\s*\*?\s+([A-Za-z_$][A-Za-z0-9_$]*)",
    )
    .expect("static");
    // a field (`pub amount: Money`), a variant (`Legacy,` / `Legacy(..)`),
    // a TS member (`oldName(): void`)
    let member = Regex::new(r"^(?:pub(?:\([^)]*\))?\s+)?([A-Za-z_$][A-Za-z0-9_$]*)\s*[:,({?]")
        .expect("static");
    let mut out = Vec::new();
    let files = crate::search::digest::respected_files_where(root, 100_000, |p| {
        p.ends_with(".rs") || crate::search::digest::ts::is_source(p)
    });
    for f in files {
        let Ok(src) = std::fs::read_to_string(root.join(&f)) else {
            continue;
        };
        if !src.contains("deprecated") {
            continue;
        }
        let lines: Vec<&str> = src.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let t = lines[i].trim();
            // the marker and where it ends: a Rust attribute may span lines
            // and carry `)` in its note (0.7.7); a doc tag is one line
            let (since, end) = if t.starts_with("#[deprecated") {
                let mut depth = 0i32;
                let mut text = String::new();
                let mut j = i;
                while j < lines.len() {
                    text.push_str(lines[j]);
                    text.push(' ');
                    depth +=
                        lines[j].matches('[').count() as i32 - lines[j].matches(']').count() as i32;
                    if depth <= 0 {
                        break;
                    }
                    j += 1;
                }
                // no `since`: listed, its window not started (0.7.7: skipped)
                (
                    Some(
                        since_re
                            .captures(&text)
                            .map_or("unversioned".to_string(), |c| c[1].to_string()),
                    ),
                    j,
                )
            } else if let Some(c) = ts_doc.captures(t) {
                // the end of the doc comment holding it
                let j = (i..lines.len())
                    .find(|&j| {
                        lines[j].contains("*/") || !lines[j].trim_start().starts_with(['*', '/'])
                    })
                    .unwrap_or(i);
                (Some(c[1].to_string()), j)
            } else {
                i += 1;
                continue;
            };
            // the item the marker stands on: the next line that is not an
            // attribute, a doc line or blank
            let name = lines
                .iter()
                .skip(end + 1)
                .map(|l| l.trim())
                .find(|l| {
                    !l.is_empty()
                        && !l.starts_with("#[")
                        && !l.starts_with("//")
                        && !l.starts_with('*')
                        && !l.starts_with("/*")
                })
                .and_then(|l| {
                    item.captures(l)
                        .or_else(|| member.captures(l))
                        .map(|c| c[1].to_string())
                });
            if let (Some(since), Some(name)) = (since, name) {
                out.push(Deprecated {
                    file: f.clone(),
                    line: i + 1,
                    name,
                    since,
                });
            }
            i = end + 1;
        }
    }
    out
}

/// Versions left before removal: the window minus minors elapsed; a new
/// major closes every window. A deprecation that has not started (a
/// `since` after the current version, or no version at all: `TBD`) has
/// its whole window left (0.7.7: those read as "closed").
pub fn left(since: &str, now: &str, window: u64) -> i64 {
    use crate::schema::store::contracts::parse_version;
    let (Some(s), Some(n)) = (parse_version(&pad(since)), parse_version(&pad(now))) else {
        return window as i64;
    };
    if s > n {
        return window as i64;
    }
    if n.0 > s.0 {
        return 0;
    }
    window as i64 - n.1.saturating_sub(s.1) as i64
}

/// `1.4` reads as `1.4.0`.
fn pad(v: &str) -> String {
    let v = v.trim();
    if v.split('-').next().unwrap_or("").matches('.').count() == 1 {
        format!("{v}.0")
    } else {
        v.to_string()
    }
}

pub fn check(root: &Path, rules: &Rules, env: Option<&str>, out: &mut Vec<Violation>) {
    let Some(rule) = rules.rules.iter().find(|r| {
        !r.is_deprecated() && r.matches_env(env) && r.check.as_deref() == Some("deprecation-window")
    }) else {
        return;
    };
    let Some(now) = project_version(root) else {
        return;
    };
    let window = rule.limit.unwrap_or(2) as u64;
    for d in scan(root) {
        if !rule.scope.is_empty()
            && !rule
                .scope
                .iter()
                .any(|s| crate::schema::model::scope::path_within(&d.file, s))
        {
            continue;
        }
        if left(&d.since, &now, window) <= 0 {
            out.push(Violation {
                check: "deprecation-window".into(),
                file: d.file.clone(),
                line: d.line,
                severity: crate::enforcement::checks::severity_of("deprecation-window"),
                message: format!(
                    "`{}` deprecated since {} — its {window}-version window closed at {now}: remove it (rule `{}`)",
                    d.name, d.since, rule.id
                ),
                hint: format!(
                    "delete it and its last callers (harna dependents {}) — {}",
                    d.file, rule.rationale
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0.7.4: Rust and TS deprecations are found; the countdown is minor
    /// versions (a major closes it); a closed window is named.
    #[test]
    fn deprecations_count_down() {
        assert_eq!(left("1.4.0", "1.4.3", 2), 2);
        assert_eq!(left("1.4.0", "1.5.0", 2), 1);
        assert_eq!(left("1.4.0", "1.6.0", 2), 0);
        assert_eq!(left("1.4.0", "2.0.0", 5), 0);
        // not started: a future since, or no version
        assert_eq!(left("2.0.0", "1.6.0", 2), 2);
        assert_eq!(left("TBD", "1.6.0", 2), 2);
        let root = std::env::temp_dir().join(format!("harna-deprec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(
            "Cargo.toml",
            "[package]\nname = \"m\"\nversion = \"1.6.2\"\n",
        );
        w(
            "src/lib.rs",
            concat!(
                "#[deprecated(\n    note = \"use total (soon)\",\n    since = \"1.4.0\"\n)]\npub fn sum() {}\n",
                "#[deprecated(since = \"1.6.0\")]\npub fn avg() {}\n",
                "pub struct S {\n    #[deprecated(since = \"1.0.0\")]\n    pub old_field: u8,\n}\n",
                "#[deprecated(since = \"TBD\")]\npub fn later() {}\n",
                "#[deprecated]\npub fn bare() {}\n",
            ),
        );
        w(
            "web/a.ts",
            "/** @deprecated since 1.3.0 */\nexport function old() {}\n",
        );
        let found = scan(&root);
        let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["sum", "avg", "old_field", "later", "bare", "old"]
        );
        let mut rules = Rules::default();
        rules.rules.push(crate::schema::model::rules::Rule {
            id: "two-minors".into(),
            check: Some("deprecation-window".into()),
            limit: Some(2),
            ..Default::default()
        });
        let mut out = Vec::new();
        check(&root, &rules, None, &mut out);
        let flagged: Vec<&str> = out
            .iter()
            .map(|v| v.message.split('`').nth(1).unwrap_or(""))
            .collect();
        assert_eq!(flagged, vec!["sum", "old_field", "old"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
