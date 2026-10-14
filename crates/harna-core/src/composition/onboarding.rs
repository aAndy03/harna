//! Onboarding: the ~1,000-token digest a fresh agent session starts with.
//!
//! One call replaces the "re-read everything after compaction" loop.

use crate::schema::manifest::Manifest;
use crate::schema::store::Store;

/// Everything a fresh session needs, pre-rendered.
#[derive(Debug, Clone)]
pub struct Onboarding {
    pub manifest_empty: bool,
    pub rule_count: usize,
    pub module_count: usize,
    pub text: String,
}

pub fn build(store: &Store) -> anyhow::Result<Onboarding> {
    let manifest = store.load_manifest().unwrap_or_default();
    let rules = store.load_rules().unwrap_or_default();
    let digest = store.load_digest().ok();

    let mut text = String::new();

    text.push_str(&arch_overview(&manifest));
    text.push_str(&rules.render_for_agent(None, None));
    if let Some(ref d) = digest {
        text.push_str(&d.render_overview(10));
    }

    Ok(Onboarding {
        manifest_empty: manifest.is_empty(),
        rule_count: rules.rules.len(),
        module_count: digest.as_ref().map(|d| d.module_count()).unwrap_or(0),
        text,
    })
}

fn arch_overview(m: &Manifest) -> String {
    if m.is_empty() {
        return "architecture: not yet recorded — call harna_log_decision as you build\n".into();
    }
    let mut out = format!(
        "architecture: {} layers, {} containers, {} components\n",
        m.layers.len(),
        m.container_count(),
        m.component_count()
    );
    for layer in &m.layers {
        out.push_str(&format!("layer [{}]\n", layer.name));
        for c in &layer.containers {
            out.push_str(&format!("  container {}\n", c.name));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::rules::Rules;
    use crate::schema::rules::Rule;

    #[test]
    fn includes_rules_and_flags_empty_manifest() {
        let tmp = std::env::temp_dir().join(format!("harna-onb-{}", std::process::id()));
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        let mut rules = Rules::default();
        rules.rules.push(Rule {
            id: "test-rule".into(),
            text: "Do the thing".into(),
            rationale: "Because".into(),
            scope: vec![],
            tags: vec![],
            origin: "test".into(),
            examples: vec![],
            env: None,
        });
        store.save_rules(&rules).unwrap();

        let onb = build(&store).unwrap();
        assert!(onb.manifest_empty);
        assert_eq!(onb.rule_count, 1);
        assert!(onb.text.contains("test-rule"));
    }
}
