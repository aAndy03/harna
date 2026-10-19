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

use crate::schema::model::manifest::Manifest;
use crate::schema::store::Store;

/// Language filler that would match everything; rejected before the search.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "how", "what", "where", "does", "from", "into", "this", "that",
    "when", "should", "make", "give", "have",
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

/// The refusal block (0.3.27): active non-goals overlapping the idea.
/// Shared by `place`/`plan` — one renderer, one message: a matching
/// refusal is not advice, it is the user's decision speaking.
pub(crate) fn refused_block(store: &Store, tokens: &[String]) -> String {
    match store.load_non_goals() {
        Ok(g) if !g.is_empty() => {
            let hits = g.matching(tokens);
            if hits.is_empty() {
                String::new()
            } else {
                let mut out = String::from("refused (non-goals in force — do not propose):\n");
                for g in hits {
                    out.push_str(&format!(
                        "  - [{}] {} — why: {}\n    supersede deliberately: the user agrees, \
                         `harna non-goal remove {}`, decision logged\n",
                        g.id, g.refusal, g.reason, g.id
                    ));
                }
                out
            }
        }
        Ok(_) => String::new(),
        Err(e) => format!(
            "refused: the decline register failed to load ({e:#}) — do NOT act unrefused; resolve .harna/non-goals.toml first\n"
        ),
    }
}

/// The prior-attempts block (0.3.27): recorded dead ends overlapping the
/// idea, strongest overlap first, capped — a fresh agent meets the
/// failure BEFORE re-proposing it.
pub(crate) fn prior_attempts_block(store: &Store, tokens: &[String]) -> String {
    const MAX_ATTEMPTS: usize = 3;
    match store.load_lessons() {
        Ok(l) if !l.is_empty() => {
            let ranked = l.prior_to(tokens, MAX_ATTEMPTS);
            if ranked.is_empty() {
                String::new()
            } else {
                let mut out = String::from(
                    "prior attempts (recorded dead ends — re-suggesting one needs a reason the old verdict no longer holds):\n",
                );
                for (lesson, hits) in ranked {
                    out.push_str(&format!(
                        "  - [{}] attempted: {}\n    verdict: {} ({} token hit(s))\n",
                        lesson.id, lesson.claim, lesson.verdict, hits
                    ));
                    if !lesson.evidence.is_empty() {
                        out.push_str(&format!(
                            "    evidence: {}\n",
                            lesson.evidence.join("; ")
                        ));
                    }
                }
                out
            }
        }
        Ok(_) => String::new(),
        Err(e) => format!(
            "prior attempts: the dead-ends register failed to load ({e:#}) — recorded failures are out of view until .harna/lessons.toml parses\n"
        ),
    }
}

/// Query-vs-non-goal: does an ACTIVE refusal flatly cover this idea?
/// Hard suppression in `plan` requires this test, and it must be exact:
/// a refused proposal is one whose tokens all appear in the refusal's
/// hay (the idea is a subset of the refusal's scope), not a one-token
/// coincidence that would suppress legitimate sibling work.
pub(crate) fn refused_by(store: &Store, tokens: &[String]) -> Vec<String> {
    match store.load_non_goals() {
        Ok(g) if !g.is_empty() => g
            .goals
            .iter()
            .filter(|ng| {
                let hay = format!("{} {}", ng.refusal, ng.reason).to_lowercase();
                tokens.iter().all(|t| hay.contains(t.as_str()))
            })
            .map(|ng| ng.id.clone())
            .collect(),
        _ => Vec::new(),
    }
}

/// The search hay for one module: its path, its public signatures AND its
/// declared item names (the inventory). Plan matching is inventory overlap —
/// a token matching a declared name hits even when no signature spells it.
pub(crate) fn module_hay(m: &crate::search::digest::Module) -> String {
    let mut names: Vec<&str> = m.signatures.iter().map(|s| s.as_str()).collect();
    names.extend(m.inventory.iter().map(|i| i.name.as_str()));
    format!("{} {}", m.path, names.join(" ")).to_lowercase()
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
            let hay = module_hay(m);
            let hits = tokens.iter().filter(|t| hay.contains(t.as_str())).count();
            if hits == 0 || matches >= 5 {
                continue;
            }
            matches += 1;
            let home = m.crate_of().0;
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

    // 3) Refusals (0.3.27): active non-goals overlapping the idea.
    out.push_str(&refused_block(store, &tokens));

    // 4) Prior attempts (0.3.27): recorded dead ends, before re-proposing.
    out.push_str(&prior_attempts_block(store, &tokens));

    // 5) Data convention reminder (constants-are-data).
    out.push_str(
        "data convention: value-set literals (lists of facts, URLs, flags) belong in data files \
         (.toml/.json/.yaml) imported from one place — `harna scaffold --kind data` generates \
         the pair; code only reads it\n",
    );

    // 6) Where to add: the engine-owning-container principle.
    out.push_str(
        "where to add: variants of an engine live inside that engine's own container \
         (modify a file or add one there, register it in the manifest); importers consume \
         the engine — never fork engine logic into the calling file\n",
    );

    // 7) Born modular (0.3.26): structure is born correct, not retro-split.
    out.push_str(
        "born modular: start per-scope — scaffold the components first, then fill them; \
         never write one oversized file intending to split it later (the splitting is \
         the writing), new files born over their budget gate at commit (`born-size`)\n",
    );

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::digest::{Digest, Module};

    fn fixture_store() -> Store {
        // Per-test dir: the place tests run in parallel and each rewrites
        // the fixture (a shared dir raced — the 0.3.32 "watch" flake).
        let tmp = std::env::temp_dir().join(format!(
            "harna-place-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
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
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![Module {
                crate_dir: None,
                src_root: None,
                path: "crates/core/src/schema/store.rs".into(),
                lang: "rust".into(),
                signatures: vec!["pub struct StoreSchema".into()],
                deps: vec![],
                body_hash: "x".into(),
                inventory: vec![],
                mods: vec![],
            }],
        };
        store
            .save_digest(&store.load_manifest().unwrap_or_default(), &digest)
            .unwrap();
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
        assert!(text.contains("born modular"));
        assert!(text.contains("born-size"));
    }

    #[test]
    fn place_surfaces_refusals_and_prior_attempts() {
        let store = fixture_store();

        // Registers empty: the blocks stay silent — no noise without data.
        let text = build(&store, "corrupt encoding in windows files").unwrap();
        assert!(!text.contains("refused"));
        assert!(!text.contains("prior attempts"));

        // Record a refusal + a dead end the idea overlaps.
        let mut g = crate::schema::registers::non_goals::NonGoals::default();
        g.upsert(crate::schema::registers::non_goals::NonGoal {
            id: "no-bulk-file-editing".into(),
            refusal: "never bulk-edit source files with PowerShell round-trips".into(),
            reason: "cp1252 misdecode corrupted 58 files in one burst".into(),
            recorded: "2026-10-17".into(),
        });
        store.save_non_goals(&g).unwrap();
        let mut l = crate::schema::registers::lessons::Lessons::default();
        l.upsert(crate::schema::registers::lessons::Lesson {
            id: "ps-roundtrip".into(),
            claim: "bulk-edit source files via PowerShell Get-Content round-trips".into(),
            verdict: "cp1252 encoding corruption across 58 files, restored byte-exact".into(),
            evidence: vec!["commit d4e81ed handoff 0.3.25".into()],
            recorded: "2026-10-17".into(),
        });
        store.save_lessons(&l).unwrap();

        let text = build(&store, "corrupt encoding in windows files").unwrap();
        assert!(text.contains("refused (non-goals in force"), "was:\n{text}");
        assert!(text.contains("[no-bulk-file-editing]"));
        assert!(text.contains("cp1252"));
        assert!(text.contains("prior attempts (recorded dead ends"));
        assert!(text.contains("[ps-roundtrip]"));
        assert!(text.contains("commit d4e81ed"));
    }

    #[test]
    fn generic_queries_are_rejected() {
        let store = fixture_store();
        assert!(build(&store, "the of and").is_err());
    }
}
