//! Component `test-kinds-edit` (0.4.9): declaring a test kind's home and
//! reach, shared by `harna test-kind` and the MCP manifest edit so the two
//! never word or behave differently.

use crate::schema::model::manifest::{Manifest, TestKind};

/// Add or replace (same name) a test kind.
pub fn add(
    m: &mut Manifest,
    name: &str,
    paths: &[String],
    reach: &[String],
    description: &str,
    contains: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    if let Some(m) = contains {
        regex::Regex::new(m).map_err(|e| {
            anyhow::anyhow!("test kind `{name}`: --contains is not a valid regex: {e}")
        })?;
    }
    if name.trim().is_empty() {
        anyhow::bail!("a test kind needs a name");
    }
    if paths.iter().all(|p| p.trim().is_empty()) {
        anyhow::bail!("test kind `{name}` needs at least one path glob, e.g. crates/*/tests/**");
    }
    let unknown: Vec<&String> = reach
        .iter()
        .filter(|r| *r != "*" && !m.layers.iter().any(|l| &l.name == *r))
        .collect();
    if !unknown.is_empty() {
        anyhow::bail!(
            "test kind `{name}`: unknown layer(s) in reach: {} (layers: {})",
            unknown
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            m.layers
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let paths: Vec<String> = paths.iter().map(|p| p.replace('\\', "/")).collect();
    m.test_kinds.retain(|k| k.name != name);
    m.test_kinds.push(TestKind {
        name: name.into(),
        paths: paths.clone(),
        reach: reach.to_vec(),
        description: description.into(),
        contains: contains.map(str::to_string),
    });
    Ok(vec![format!(
        "test kind `{name}` covers {} and may reach {} — validate holds its files and dev-dependencies to that (test-reach)",
        paths.join(", "),
        if reach.is_empty() { "nothing beyond its home layer".into() } else { reach.join(", ") }
    )])
}

pub fn remove(m: &mut Manifest, name: &str) -> anyhow::Result<Vec<String>> {
    let before = m.test_kinds.len();
    m.test_kinds.retain(|k| k.name != name);
    if m.test_kinds.len() == before {
        anyhow::bail!("no test kind `{name}`");
    }
    Ok(vec![format!("test kind `{name}` removed")])
}

#[cfg(test)]
mod tests;
