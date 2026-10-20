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
    let rust_attr = Regex::new(r#"#\[deprecated\([^)]*since\s*=\s*"([^"]+)""#).expect("static");
    let ts_doc = Regex::new(r"@deprecated\s+(?:since\s+)?v?(\d+\.\d+(?:\.\d+)?)").expect("static");
    let name = Regex::new(
        r"\b(?:fn|struct|enum|trait|type|const|static|mod|function|class|interface)\s+([A-Za-z_$][A-Za-z0-9_$]*)",
    )
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
        for (i, l) in lines.iter().enumerate() {
            let since = rust_attr
                .captures(l)
                .or_else(|| ts_doc.captures(l))
                .map(|c| c[1].to_string());
            let Some(since) = since else { continue };
            // the item the marker stands before
            if let Some(n) = lines
                .iter()
                .skip(i)
                .take(8)
                .find_map(|x| name.captures(x).map(|c| c[1].to_string()))
            {
                out.push(Deprecated {
                    file: f.clone(),
                    line: i + 1,
                    name: n,
                    since,
                });
            }
        }
    }
    out
}

/// Versions left before removal: the window minus minors elapsed (a new
/// major closes every window).
pub fn left(since: &str, now: &str, window: u64) -> i64 {
    let (sa, sb, _) = crate::schema::store::contracts::semver(since);
    let (na, nb, _) = crate::schema::store::contracts::semver(now);
    if na > sa {
        return 0;
    }
    window as i64 - nb.saturating_sub(sb) as i64
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
            "#[deprecated(since = \"1.4.0\", note = \"use total\")]\npub fn sum() {}\n#[deprecated(since = \"1.6.0\")]\npub fn avg() {}\n",
        );
        w(
            "web/a.ts",
            "/** @deprecated since 1.3.0 */\nexport function old() {}\n",
        );
        let found = scan(&root);
        let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["sum", "avg", "old"]);
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
        assert_eq!(flagged, vec!["sum", "old"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
