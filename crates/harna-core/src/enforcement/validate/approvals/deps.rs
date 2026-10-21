//! The dependencies a manifest declares (1.1.4): Cargo.toml tables and
//! package.json maps, by name. Text in, names out — nothing is resolved.

use std::collections::BTreeSet;

/// Is this a dependency manifest the check reads?
pub fn is_manifest(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "Cargo.toml" || name == "package.json"
}

/// The dependency names `text` declares; empty when it does not parse.
pub fn names(path: &str, text: &str) -> BTreeSet<String> {
    if path.ends_with("package.json") {
        npm(text)
    } else {
        cargo(text)
    }
}

fn npm(text: &str) -> BTreeSet<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return BTreeSet::new();
    };
    [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ]
    .iter()
    .filter_map(|k| v.get(k).and_then(|d| d.as_object()))
    .flat_map(|d| d.keys().cloned())
    .collect()
}

fn cargo(text: &str) -> BTreeSet<String> {
    let Ok(v) = toml::from_str::<toml::Value>(text) else {
        return BTreeSet::new();
    };
    let tables = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut out = BTreeSet::new();
    let mut take = |t: &toml::Value| {
        for k in tables {
            if let Some(d) = t.get(k).and_then(|d| d.as_table()) {
                out.extend(d.keys().cloned());
            }
        }
    };
    take(&v);
    if let Some(w) = v.get("workspace") {
        take(w);
    }
    // [target.'cfg(..)'.dependencies]
    if let Some(targets) = v.get("target").and_then(|t| t.as_table()) {
        for t in targets.values() {
            take(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_and_npm_dependencies_are_named() {
        let cargo = "[package]\nname = \"x\"\n[dependencies]\nserde = \"1\"\n[dev-dependencies]\ninsta = \"1\"\n[workspace.dependencies]\ntokio = \"1\"\n[target.'cfg(windows)'.dependencies]\nwinapi = \"0.3\"\n";
        let n: Vec<String> = names("Cargo.toml", cargo).into_iter().collect();
        assert_eq!(n, vec!["insta", "serde", "tokio", "winapi"]);
        let npm = r#"{"name":"w","dependencies":{"react":"18"},"devDependencies":{"vitest":"1"}}"#;
        let n: Vec<String> = names("web/package.json", npm).into_iter().collect();
        assert_eq!(n, vec!["react", "vitest"]);
        assert!(names("Cargo.toml", "not toml [").is_empty());
        assert!(is_manifest("crates/a/Cargo.toml") && !is_manifest("Cargo.lock"));
    }
}
