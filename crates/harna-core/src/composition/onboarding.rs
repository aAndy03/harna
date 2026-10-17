//! Onboarding: the ~1,000-token digest a fresh agent session starts with.
//!
//! One call replaces the "re-read everything after compaction" loop.

use crate::schema::manifest::Manifest;
use crate::schema::store::Store;
use crate::search::digest::{render_overview_from, Module};

/// How many leading modules the overview details before the "+N more"
/// line (same number `Digest::render_overview` callers passed).
const OVERVIEW_MODULES: usize = 10;

/// The map's overview data, accumulated under a bounded stream (0.3.16):
/// totals from the whole pass, detail only for the leading modules —
/// shards visit one container at a time and nothing else stays resident.
struct MapSnapshot {
    generated_at: String,
    modules: usize,
    signatures: usize,
    first: Vec<Module>,
}

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

    // Bounded map read (0.3.16): counts ride the whole stream, the detail
    // list caps at OVERVIEW_MODULES.
    let mut map: Option<MapSnapshot> = None;
    store
        .for_each_shard(|stamp, shard| {
            let entry = map.get_or_insert_with(|| MapSnapshot {
                generated_at: stamp.generated_at.clone(),
                modules: 0,
                signatures: 0,
                first: Vec::new(),
            });
            for m in &shard.modules {
                entry.modules += 1;
                entry.signatures += m.signatures.len();
                if entry.first.len() < OVERVIEW_MODULES {
                    entry.first.push(m.clone());
                }
            }
            true
        })
        .ok(); // unreadable map = no map (same as the old `load_digest().ok()`)

    let mut text = String::new();

    text.push_str(&arch_overview(&manifest));
    text.push_str(&rules.render_for_agent(None, None));
    text.push_str(&terms_brief(store));
    if let Some(snap) = &map {
        text.push_str(&render_overview_from(
            &snap.generated_at,
            snap.modules,
            snap.signatures,
            &snap.first,
            OVERVIEW_MODULES,
        ));
    }
    text.push_str(&parity_summary(store));
    // The burst journal: a fresh (or resuming) session first sees where the
    // last burst stopped.
    text.push_str(&crate::schema::handoff::onboarding_text(store));

    Ok(Onboarding {
        manifest_empty: manifest.is_empty(),
        rule_count: rules.rules.len(),
        module_count: map.as_ref().map(|s| s.modules).unwrap_or(0),
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
        if layer.sequence.is_empty() {
            out.push_str("  order: unordered (layer.sequence empty)\n");
        } else {
            out.push_str(&format!("  boot: {}\n", layer.sequence.join(" -> ")));
        }
        for c in &layer.containers {
            let seq = if c.sequence.is_empty() {
                String::new()
            } else {
                format!(" [{}]", c.sequence.join(", "))
            };
            out.push_str(&format!("  container {}{}\n", c.name, seq));
        }
    }
    out
}

/// The glossary in compressed form (0.3.26): agents carry it in every
/// session, so it renders bounded — the first MAX_TERMS entries detail,
/// the rest count. A missing file is nothing; an unparseable one is
/// named (a silent glossary would teach the wrong vocabulary).
fn terms_brief(store: &Store) -> String {
    const MAX_TERMS: usize = 16;
    match store.load_terms() {
        Ok(t) if !t.is_empty() => {
            let mut out = String::from(
                "glossary: harna's vocabulary (`.harna/terms.toml`) — an agent's own words may \
                 map differently; these are harna's meanings\n",
            );
            for term in t.terms.iter().take(MAX_TERMS) {
                let aliases = if term.aliases.is_empty() {
                    String::new()
                } else {
                    format!(" (also: {})", term.aliases.join(", "))
                };
                out.push_str(&format!("  {}{} = {}\n", term.word, aliases, term.means));
            }
            let rest = t.terms.len().saturating_sub(MAX_TERMS);
            if rest > 0 {
                out.push_str(&format!("  … +{rest} more terms (read .harna/terms.toml)\n"));
            }
            out
        }
        Ok(_) => String::new(),
        Err(_) => "glossary: .harna/terms.toml is unparseable — fix it (doctor names the damage)\n"
            .into(),
    }
}

fn parity_summary(store: &Store) -> String {
    // Composition may reach into any engine container.
    match crate::enforcement::parity::list_events(store) {
        Ok(events) if !events.is_empty() => {
            let mut out = String::new();
            for e in &events {
                let open = e.open_targets().len();
                if open > 0 {
                    out.push_str(&format!(
                        "open parity debt: {} targets in `{}` (gen{}) — acknowledge before commit\n",
                        open, e.id, e.generation
                    ));
                }
            }
            out
        }
        _ => String::new(),
    }
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
            check: None,
            severity: None,
            pattern: None,
            limit: None,
            status: None,
        });
        store.save_rules(&rules).unwrap();

        let onb = build(&store).unwrap();
        assert!(onb.manifest_empty);
        assert_eq!(onb.rule_count, 1);
        assert!(onb.text.contains("test-rule"));

        // The glossary rides onboarding when recorded: a fresh session
        // meets harna's vocabulary without a separate query.
        let terms = crate::schema::terms_data::Terms {
            schema_version: crate::SCHEMA_VERSION,
            terms: vec![crate::schema::terms_data::Term {
                word: "engine".into(),
                aliases: vec!["service".into()],
                means: "a role-carrying container hanging off a layer".into(),
            }],
        };
        store.save_terms(&terms).unwrap();
        let onb2 = build(&store).unwrap();
        assert!(onb2.text.contains("glossary:"));
        assert!(onb2.text.contains("engine (also: service) = a role-carrying"));

        // A recorded handoff arrives through the same call — the resumed
        // session does not need a separate query.
        crate::schema::handoff::save(
            &store,
            "pre-compaction burst",
            &["wire the demo".to_string()],
            0,
        )
        .unwrap();
        let onb = build(&store).unwrap();
        assert!(onb.text.contains("pre-compaction burst"));
        assert!(onb.text.contains("1. wire the demo"));
    }

    #[test]
    fn the_map_overview_is_bounded_and_identical_to_the_whole_map_era() {
        // The bounded read (0.3.16): totals ride the whole stream, detail
        // caps at ten — and the rendered text equals the whole-map era's
        // `render_overview`, byte for byte.
        use crate::search::digest::{Digest, Module};

        let tmp = std::env::temp_dir().join(format!("harna-onb-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        let mk = |i: usize| Module {
            path: format!("src/m{i}.rs"),
            lang: "rust".into(),
            signatures: vec![format!("pub fn f{i}()")],
            deps: vec![],
            body_hash: String::new(),
            inventory: vec![],
            mods: vec![],
        };
        let modules: Vec<Module> = (0..14).map(mk).collect();
        let d = Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules,
        };
        store
            .save_digest(&store.load_manifest().unwrap_or_default(), &d)
            .unwrap();

        let onb = build(&store).unwrap();
        assert_eq!(onb.module_count, 14);
        assert!(onb.text.contains("codebase map: 14 modules, 14 signatures"));
        assert!(onb.text.contains("src/m0.rs"), "detail rides the leading modules");
        assert!(!onb.text.contains("src/m10.rs"), "detail caps before the tail");
        assert!(onb.text.contains("… +4 more modules"));

        // Byte-identical to the whole-map renderer — the delegation holds.
        let expected = crate::search::digest::render_overview_from(
            "now",
            14,
            14,
            &d.modules[..10.min(d.modules.len())],
            10,
        );
        assert!(
            onb.text.contains(expected.trim_end()),
            "overview section must equal the whole-map rendering"
        );
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    }
}
