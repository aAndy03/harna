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

/// Planned slots whose name or purpose overlaps the idea (0.4.1): the
/// future was given a home before its code; build it there.
/// Planned slots and homes by purpose; with no code map and nothing that
/// fits, the declared layers to choose from (1.0 review: a new repository
/// with no matching description got no placement advice at all).
pub(crate) fn homes_block(
    store: &Store,
    manifest: &Manifest,
    tokens: &[String],
    map_present: bool,
) -> String {
    let idea =
        super::purpose::idea_stems(tokens, &store.load_effective_terms().unwrap_or_default());
    let found = format!(
        "{}{}",
        slots_block(manifest, tokens),
        super::purpose::homes_block(manifest, &idea)
    );
    if !found.is_empty() || map_present {
        return found;
    }
    let layers: Vec<String> = manifest
        .layers
        .iter()
        .map(|l| {
            let cs: Vec<&str> = l.containers.iter().map(|c| c.name.as_str()).collect();
            if cs.is_empty() {
                l.name.clone()
            } else {
                format!("{} ({})", l.name, cs.join(", "))
            }
        })
        .collect();
    if layers.is_empty() {
        "homes: none declared yet — pick a layer by responsibility, then `harna container add <name> --layer <layer> --path <dir>`\n".into()
    } else {
        format!(
            "homes: no declared description fits — pick by responsibility among: {}; give containers a --description so the next ask finds them\n",
            layers.join("; ")
        )
    }
}

pub(crate) fn slots_block(manifest: &Manifest, tokens: &[String]) -> String {
    let hits: Vec<String> = crate::generation::structure::edit::slots::slots(manifest)
        .into_iter()
        .filter(|(_, _, s)| {
            let hay = format!("{} {} {}", s.name, s.summary, s.path).to_lowercase();
            tokens.iter().any(|t| hay.contains(t.as_str()))
        })
        .map(|(l, c, s)| {
            format!(
                "  - `{}` [layer {l}, container {c}] at {} — build it there: `harna scaffold --layer {l} --container {c} --name {} --kind primitive`\n",
                s.name, s.path, s.name
            )
        })
        .collect();
    if hits.is_empty() {
        String::new()
    } else {
        format!(
            "planned slots that fit (declared homes for this idea):\n{}",
            hits.concat()
        )
    }
}

/// The qualities block (0.4.0): system-wide qualities plus those scoped
/// to `paths`. Shared by onboarding (no paths), place and plan.
pub(crate) fn qualities_block(store: &Store, paths: &[&str], heading: &str) -> String {
    match store.load_qualities() {
        Ok(q) if !q.is_empty() => {
            let hits = q.in_scope(paths);
            if hits.is_empty() {
                return String::new();
            }
            let mut out = format!("{heading}:\n");
            for h in hits {
                out.push_str(&format!("  - {}\n", h.line()));
            }
            out
        }
        Ok(_) => String::new(),
        Err(e) => format!("qualities: .harna/qualities.toml failed to load ({e:#}) — long-term intent is out of view\n"),
    }
}

/// The rules a briefing shows (0.3.50): global ones and those whose scope
/// covers a focus container — not every rule of the repo. No focus (a
/// brand-new placement) shows them all.
/// The rules in force here by id (0.9.5): the session summary already
/// carries each statement, `harna onboarding rules` each why — place and
/// plan only say which ones apply.
pub(crate) fn rules_line(rules: &crate::schema::model::rules::Rules) -> String {
    let ids: Vec<&str> = rules
        .rules
        .iter()
        .filter(|r| !r.is_deprecated())
        .map(|r| r.id.as_str())
        .collect();
    if ids.is_empty() {
        return "rules to respect: none recorded\n".into();
    }
    format!(
        "rules to respect here ({}; statements in the session summary, why: harna onboarding rules): {}\n",
        ids.len(),
        ids.join(", ")
    )
}

pub(crate) fn relevant_rules(
    manifest: &Manifest,
    rules: &crate::schema::model::rules::Rules,
    focus: &[String],
) -> crate::schema::model::rules::Rules {
    use crate::schema::model::scope::path_within;
    let paths: Vec<&str> = manifest
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .filter(|c| focus.contains(&c.name) && !c.path.is_empty())
        .map(|c| c.path.as_str())
        .collect();
    let mut relevant = rules.clone();
    // deprecated rules stay visible in the file, not in briefings
    relevant.rules.retain(|r| !r.is_deprecated());
    if !paths.is_empty() {
        relevant.rules.retain(|r| {
            r.scope.is_empty()
                || r.scope
                    .iter()
                    .any(|s| paths.iter().any(|p| path_within(p, s) || path_within(s, p)))
        });
    }
    relevant
}

pub fn build(store: &Store, query: &str) -> anyhow::Result<String> {
    let manifest: Manifest = store.load_manifest().unwrap_or_default();
    let rules = store.load_effective_rules().unwrap_or_default();
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
    // The containers the similar modules live in: the briefing's focus.
    let mut focus: Vec<String> = Vec::new();
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
            if container != "(none)" && !focus.contains(&container) {
                focus.push(container.clone());
            }
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
        out.push_str("similar: no code map yet — the homes below come from the declared containers (`harna scan` once code exists finds similar code)\n");
    }

    // 1b) Planned slots that fit (0.4.1), then homes by declared purpose
    // (0.4.4): the right container even before any code mentions the idea.
    out.push_str(&homes_block(store, &manifest, &tokens, digest.is_some()));

    // 2) Which rules apply? Global ones and those whose scope covers a
    // focus container (0.3.50, ledger field test: every rule was printed).
    out.push_str(&rules_line(&relevant_rules(&manifest, &rules, &focus)));

    // 2a) Qualities the touched code must hold (0.4.0).
    let focus_paths: Vec<&str> = manifest
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .filter(|c| focus.contains(&c.name) && !c.path.is_empty())
        .map(|c| c.path.as_str())
        .collect();
    out.push_str(&qualities_block(
        store,
        &focus_paths,
        "qualities to hold here",
    ));

    // 2b) Tools that fit the focus (0.3.50): offered, never required.
    let focus_refs: Vec<&str> = focus.iter().map(String::as_str).collect();
    out.push_str(&super::toolbox::suggestions(
        &store.repo_root(),
        &manifest,
        &rules,
        digest.as_ref(),
        &focus_refs,
    ));

    // 3) Refusals (0.3.27): active non-goals overlapping the idea.
    out.push_str(&refused_block(store, &tokens));

    // 4) Prior attempts (0.3.27): recorded dead ends, before re-proposing.
    out.push_str(&prior_attempts_block(store, &tokens));

    // 5) The conventions, in one line (1.0); the paragraphs are one
    // `harna onboarding conventions` away.
    out.push_str(super::plan::CONVENTIONS_LINE);

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
            orphans: Vec::new(),
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![Module {
                reexports: Vec::new(),
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
        assert!(text.contains("variants live in their engine's container"));
        assert!(text.contains("born modular"));
        assert!(text.contains("harna onboarding conventions"));
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

    /// 0.4.0: what the software must hold reaches every session —
    /// onboarding shows the system-wide qualities; a briefing adds those
    /// scoped to the code it focuses on.
    #[test]
    fn qualities_reach_onboarding_and_briefings() {
        use crate::schema::registers::qualities::{Qualities, Quality};
        let store = fixture_store();
        let mut q = Qualities::default();
        for (id, scope) in [
            ("scale-users", vec![]),
            ("store-durable", vec!["src/data".to_string()]),
        ] {
            q.upsert(Quality {
                held_by: Vec::new(),
                id: id.into(),
                statement: format!("{id} holds"),
                measure: if id == "scale-users" {
                    "1M users".into()
                } else {
                    String::new()
                },
                category: String::new(),
                scope,
                why: String::new(),
                recorded: String::new(),
            })
            .unwrap();
        }
        store.save_qualities(&q).unwrap();
        let onb = crate::composition::briefing::onboarding::build(&store)
            .unwrap()
            .text;
        assert!(
            onb.contains("[scale-users] scale-users holds (1M users)"),
            "{onb}"
        );
        assert!(
            !onb.contains("store-durable"),
            "scoped qualities wait for their code"
        );
        let block = qualities_block(&store, &["src/data/users"], "qualities to hold here");
        assert!(
            block.contains("store-durable") && block.contains("scale-users"),
            "{block}"
        );
    }
}
