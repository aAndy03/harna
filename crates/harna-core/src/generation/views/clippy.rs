//! `clippy` view (0.3.45, deno study B3): `banned-api` rules written into
//! each covered crate's `clippy.toml` as `disallowed-methods`, so the
//! compiler enforces what the rule declares.
//!
//! A crate is covered when a rule's scope covers its whole directory (or
//! the rule is global) — a scope narrower than a crate stays a textual
//! check only; clippy config is crate-wide and must not over-ban.
//! Existing `clippy.toml` content is kept: other keys untouched, other
//! `disallowed-methods` entries kept, harna's added once by `path`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::schema::model::rules::Rules;
use crate::schema::model::scope::path_within;
use crate::search::digest::crates::CrateIndex;

/// One ban as clippy reads it: path, reason, sanctioned replacement.
pub type Ban = (String, String, Option<String>);

/// crate dir -> the bans the rules require there.
pub fn required(root: &Path, rules: &Rules) -> BTreeMap<String, Vec<Ban>> {
    let index = CrateIndex::load(root);
    let mut out: BTreeMap<String, Vec<Ban>> = BTreeMap::new();
    for r in rules
        .rules
        .iter()
        .filter(|r| !r.is_deprecated() && r.check.as_deref() == Some("banned-api"))
    {
        for c in &index.crates {
            let covered = r.scope.is_empty() || r.scope.iter().any(|s| path_within(&c.dir, s));
            if !covered {
                continue;
            }
            let entry = out.entry(c.dir.clone()).or_default();
            for v in &r.values {
                let (path, to) = crate::schema::model::rules::value_and_replacement(v);
                if !entry.iter().any(|(p, _, _)| p == path) {
                    entry.push((
                        path.to_string(),
                        format!("{} (harna rule `{}`)", r.rationale, r.id),
                        to.map(str::to_string),
                    ));
                }
            }
        }
    }
    out
}

/// Paths a crate's clippy.toml is missing, per crate.
pub fn drift(root: &Path, rules: &Rules) -> BTreeMap<String, Vec<String>> {
    required(root, rules)
        .into_iter()
        .filter_map(|(dir, bans)| {
            let have = present(&root.join(&dir).join("clippy.toml"));
            let missing: Vec<String> = bans
                .into_iter()
                .map(|(p, _, _)| p)
                .filter(|p| !have.contains(p))
                .collect();
            (!missing.is_empty()).then_some((dir, missing))
        })
        .collect()
}

/// Write the bans; returns the clippy.toml paths it changed.
pub fn generate(root: &Path, rules: &Rules) -> anyhow::Result<Vec<String>> {
    let mut written = Vec::new();
    for (dir, bans) in required(root, rules) {
        let path = root.join(&dir).join("clippy.toml");
        let mut doc: toml::Table = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();
        let list = doc
            .entry("disallowed-methods")
            .or_insert_with(|| toml::Value::Array(Vec::new()));
        let Some(arr) = list.as_array_mut() else {
            anyhow::bail!("{}: `disallowed-methods` is not a list", path.display());
        };
        let mut changed = false;
        for (p, reason, replacement) in bans {
            let exists = arr.iter().any(|e| {
                e.as_str() == Some(p.as_str())
                    || e.get("path").and_then(|x| x.as_str()) == Some(p.as_str())
            });
            if !exists {
                let mut t = toml::Table::new();
                t.insert("path".into(), toml::Value::String(p));
                t.insert("reason".into(), toml::Value::String(reason));
                if let Some(to) = replacement {
                    t.insert("replacement".into(), toml::Value::String(to));
                }
                arr.push(toml::Value::Table(t));
                changed = true;
            }
        }
        if changed {
            crate::schema::store::atomic::write(&path, toml::to_string_pretty(&doc)?)?;
            written.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
        }
    }
    Ok(written)
}

fn present(path: &Path) -> Vec<String> {
    let Some(doc) = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str::<toml::Table>(&s).ok())
    else {
        return Vec::new();
    };
    doc.get("disallowed-methods")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    e.as_str()
                        .or_else(|| e.get("path").and_then(|x| x.as_str()))
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
