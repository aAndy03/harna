//! Component `place`: guidance before code.
//!
//! The PLAN step of harna's flow, in its 0.2 form: given an idea in plain
//! words, return the placement map — where similar things live (digest),
//! which layers/containers host them (manifest), which rules apply (rules),
//! and where the value-sets belong (constants-are-data). The agent places
//! the variant inside the engine's own container because harna said where
//! home is *before* the first line was written. The cram signal only
//! advises afterward — never polices.

use std::collections::BTreeMap;

use crate::schema::manifest::Manifest;
use crate::schema::store::Store;

/// Language filler that would match everything; rejected before the search.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "how", "what", "where", "does", "from",
    "into", "this", "that", "when", "should", "make", "give", "have",
];

/// Idea -> meaningful tokens: shared by the placement map and the full
/// plan briefing, so both match the same way.
pub(crate) fn tokenize(query: &str) -> anyhow::Result<Vec<String>> {
    let tokens: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .filter(|t| t.len() > 2 && !STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect();
    if tokens.is_empty() {
        anyhow::bail!("query too generic — pass an idea like 'timezone picker data'");
    }
    Ok(tokens)
}

pub fn build(store: &Store, query: &str) -> anyhow::Result<String> {
    let manifest: Manifest = store.load_manifest().unwrap_or_default();
    let rules = store.load_rules().unwrap_or_default();
    let digest = store.load_digest().ok();

    let tokens = tokenize(query)?;

    // Container directory -> (layer, container) understood by the manifest.
    let mut dirs: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for l in &manifest.layers {
        for c in &l.containers {
            dirs.insert(c.path.as_str(), (l.name.as_str(), c.name.as_str()));
        }
    }

    let mut out = String::new();
    out.push_str(&format!("placement briefing for `{query}`:\n"));

    // 1) Where do similar things live?
    let mut matches = 0;
    if let Some(d) = &digest {
        for m in &d.modules {
            let hay = format!("{} {}", m.path, m.signatures.join(" ")).to_lowercase();
            let hits = tokens.iter().filter(|t| hay.contains(t.as_str())).count();
            if hits == 0 || matches >= 5 {
                continue;
            }
            matches += 1;
            let home = crate::search::digest::crate_of(&m.path).0;
            let home = home.unwrap_or_default();
            let (layer, container) = dirs
                .iter()
                .find(|(path, _)| m.path.starts_with(*path))
                .map(|(_, v)| (v.0.to_string(), v.1.to_string()))
                .unwrap_or_else(|| ("(ungoverned)".into(), "(none)".into()));
            out.push_str(&format!(
                "similar: {} [layer {}, container {}]\n",
                m.path, layer, container
            ));
            for sig in m.signatures.iter().take(3) {
                out.push_str(&format!("    {sig}\n"));
            }
            let _ = home;
        }
        if matches == 0 {
            out.push_str("similar: none found — new placement; pick the layer by responsibility and record it in the manifest\n");
        }
    } else {
        out.push_str("similar: CODE MAP MISSING — run `harna scan` first, then re-ask (placement needs the map)\n");
    }

    // 2) Which rules apply?
    let mut rule_lines = rules.render_for_agent(None, None);
    if !rule_lines.trim().is_empty() && !rule_lines.contains("No rules recorded") {
        rule_lines = rule_lines
            .lines()
            .filter(|l| !l.starts_with("  scope:") && !l.starts_with("  env:"))
            .collect::<Vec<_>>()
            .join("\n");
        out.push_str(&format!("rules to respect:\n{rule_lines}\n"));
    }

    // 3) Data convention reminder (constants-are-data).
    out.push_str(
        "data convention: value-set literals (lists of facts, URLs, flags) belong in data files \
         (.toml/.json/.yaml) imported from one place — `harna scaffold --kind data` generates \
         the pair; code only reads it\n",
    );

    // 4) Where to add: the engine-owning-container principle.
    out.push_str(
        "where to add: variants of an engine live inside that engine's own container \
         (modify a file or add one there, register it in the manifest); importers consume \
         the engine — never fork engine logic into the calling file\n",
    );

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::digest::{Digest, Module};

    fn fixture_store() -> Store {
        let tmp = std::env::temp_dir().join(format!("harna-place-{}", std::process::id()));
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        let manifest_src = r#"
schema_version = 1

[[layers]]
name = "memory"

[[layers.containers]]
name = "schema"
path = "crates/core/src/schema"
"#;
        std::fs::write(store.manifest_path(), manifest_src).unwrap();

        let rules_src = r#"
schema_version = 1

[[rules]]
id = "constants-are-data"
text = "Value-set literals live in data files, imported once"
rationale = "One source beats four copies"
scope = ["crates/core"]
origin = "test"
"#;
        std::fs::write(store.rules_path(), rules_src).unwrap();

        let digest = Digest {
            generated_at: "now".into(),
            modules: vec![Module {
                path: "crates/core/src/schema/store.rs".into(),
                lang: "rust".into(),
                signatures: vec!["pub struct StoreSchema".into()],
                deps: vec![],
                body_hash: "x".into(),
            inventory: vec![],
            mods: vec![],
        },]
        };
        store.save_digest(&digest).unwrap();
        store
    }

    #[test]
    fn place_returns_map_rules_and_conventions() {
        let store = fixture_store();
        let text = build(&store, "where does store schema live").unwrap();
        assert!(text.contains("crates/core/src/schema/store.rs"));
        assert!(text.contains("[layer memory, container schema]"));
        assert!(text.contains("constants-are-data"));
        assert!(text.contains("data files"));
        assert!(text.contains("inside that engine's own container"));
    }

    #[test]
    fn generic_queries_are_rejected() {
        let store = fixture_store();
        assert!(build(&store, "the of and").is_err());
    }
}
