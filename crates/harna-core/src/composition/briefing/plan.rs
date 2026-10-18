//! Component `plan`: the pre-code briefing.
//!
//! The PLAN step in its 0.3 form: one briefing that reads as a decision
//! proposal — placement (where similar things live, which containers to
//! extend), decision context (supersession-aware: superseded decisions are
//! named visibly dead, never followed), the rules that will constrain the
//! change, the dependency fan-out to expect (dependents), and the test
//! obligations to honor (registered tests + the no-test-update advisory).
//! The PLAN step being as explicit as the CHECK step means an agent runs
//! this before the first line and plans around obligations it has already
//! seen enumerated.

use crate::schema::store::Store;

const DATA_CONVENTION: &str = "data convention: value-set literals (lists of facts, URLs, flags) belong in data files (.toml/.json/.yaml) imported from one place — `harna scaffold --kind data` generates the pair; code only reads it";

const ADD_PRINCIPLE: &str = "where to add: variants of an engine live inside that engine's own container (modify a file or add one there, register it in the manifest); importers consume the engine — never fork engine logic into the calling file";

/// Born modular (0.3.26): structure is born correct, not retro-split —
/// the planning briefing says so BEFORE the first line exists.
const BORN_MODULAR: &str = "born modular: start per-scope — scaffold the components first, then fill them; never write one oversized file intending to split it later (the splitting is the writing), new files born over their budget gate at commit (`born-size`)";

const TEST_OBLIGATION: &str = "test obligation: components register their tests in the manifest; a changed component whose registered tests did not change in the same burst gets the `no-test-update` advisory (validate --changed) — extend the tests or record why in the decision log. Registered test paths must exist and hold real test code (`tests-declared`)";

const MAX_SIMILAR: usize = 5;
const MAX_DECISIONS: usize = 3;
const MAX_DEPENDENTS_VIEWS: usize = 2;

pub fn build(store: &Store, query: &str) -> anyhow::Result<String> {
    let manifest = store.load_manifest().unwrap_or_default();
    let rules = store.load_rules().unwrap_or_default();
    let tokens = super::place::tokenize(query)?;

    // Container directory: (path, layer, container) sorted for stable lookup.
    let mut dirs: Vec<(&str, &str, &str)> = manifest
        .layers
        .iter()
        .flat_map(|l| {
            l.containers
                .iter()
                .map(|c| (c.path.as_str(), l.name.as_str(), c.name.as_str()))
        })
        .collect();
    dirs.sort();

    let mut out = String::new();
    out.push_str(&format!(
        "plan briefing for `{query}` — plan -> place -> code -> check -> align -> commit:\n"
    ));

    // 0) Hard refusals (0.3.27): an active non-goal covering the ENTIRE
    // idea's token set suppresses planning outright. The refusal speaks,
    // and the briefing becomes the supersession instructions — placement,
    // candidates and rules would just decorate a forbidden proposal.
    let refused = super::place::refused_by(store, &tokens);
    if !refused.is_empty() {
        out.push_str("REFUSED: an active non-goal covers this whole idea — no plan is offered:\n");
        if let Ok(g) = store.load_non_goals() {
            for ng in &g.goals {
                if refused.contains(&ng.id) {
                    out.push_str(&format!(
                        "  - [{}] {}\n    why: {}\n",
                        ng.id, ng.refusal, ng.reason
                    ));
                }
            }
        }
        out.push_str(
            "  to proceed the user must agree consciously: revisit with the user, then \
             `harna non-goal remove <id>` and log a superseding decision \
             (`harna log-decision`); planning a refused proposal quietly is how agents \
             betray the human (q7)\n",
        );
        out.push_str(&super::place::prior_attempts_block(store, &tokens));
        return Ok(out);
    }

    // 1) Placement: where similar things live, what to extend.
    let mut similar: Vec<(String, Option<(&str, &str)>)> = Vec::new();
    if let Ok(digest) = store.load_digest() {
        for m in &digest.modules {
            if similar.len() >= MAX_SIMILAR {
                break;
            }
            let hay = super::place::module_hay(m);
            if tokens.iter().filter(|t| hay.contains(t.as_str())).count() == 0 {
                continue;
            }
            let home = dirs
                .iter()
                .find(|(path, _, _)| m.path.starts_with(path))
                .map(|(_, l, c)| (*l, *c));
            similar.push((m.path.clone(), home));
            out.push_str(&format!(
                "similar: {} [layer {}, container {}]\n",
                m.path,
                home.map_or("(ungoverned)", |(l, _)| l),
                home.map_or("(none)", |(_, c)| c),
            ));
            for sig in m.signatures.iter().take(3) {
                out.push_str(&format!("    {sig}\n"));
            }
        }
    } else {
        out.push_str(
            "similar: CODE MAP MISSING — run `harna scan` first, then re-ask (planning needs the map)\n",
        );
    }
    if similar.is_empty() {
        out.push_str("similar: none found — new placement; pick the layer by responsibility and record it in the manifest\n");
    }
    let mut hosts: Vec<String> = similar
        .iter()
        .filter_map(|(_, home)| home.map(|(l, c)| format!("{c} (layer {l})")))
        .collect();
    hosts.sort();
    hosts.dedup();
    if !hosts.is_empty() {
        out.push_str(&format!("candidates to extend: {}\n", hosts.join(", ")));
    }

    // 2) Decision context: relevant decisions, supersession-aware.
    let decisions = crate::schema::journal::decisions::list(&store.decisions_dir()).unwrap_or_default();
    let mut decision_hits: Vec<&crate::schema::journal::decisions::Decision> = Vec::new();
    for d in &decisions {
        if decision_hits.len() >= MAX_DECISIONS {
            break;
        }
        let hay = format!("{} {}", d.title, d.supersedes.clone().unwrap_or_default()).to_lowercase();
        if tokens.iter().any(|t| hay.contains(t.as_str())) {
            decision_hits.push(d);
        }
    }
    if decision_hits.is_empty() {
        out.push_str("decision context: none matched — historic guidance has nothing specific to say\n");
    } else {
        out.push_str("decision context:\n");
        for d in decision_hits {
            if let Some(s) = &d.supersedes {
                out.push_str(&format!("  - {} — supersedes {s}\n", d.title));
            } else if crate::schema::journal::decisions::is_superseded(&d.file, &decisions) {
                let successor = decisions
                    .iter()
                    .find(|x| x.supersedes.as_deref() == Some(d.file.as_str()))
                    .map(|x| x.file.as_str())
                    .unwrap_or("?");
                out.push_str(&format!(
                    "  - {} — VISIBLY DEAD: superseded by {successor}; do not follow\n",
                    d.title
                ));
            } else {
                out.push_str(&format!("  - {}\n", d.title));
            }
        }
    }

    // 3) Rules that will constrain/consequence the change.
    if rules.rules.is_empty() {
        out.push_str("rules to respect: none recorded\n");
    } else {
        out.push_str("rules to respect (will gate or annotate — bounds are check ids):\n");
        for r in &rules.rules {
            if r.status.as_deref() == Some("deprecated") {
                out.push_str(&format!("  - {} — (deprecated; not in force)\n", r.id));
                continue;
            }
            let binding = r.check.as_ref().map_or(String::new(), |c| format!(" [check: {c}]"));
            out.push_str(&format!("  - {}: {}{}\n", r.id, r.text, binding));
        }
    }

    // 3b) Decline context (0.3.27): partial-refusal overlap and prior
    // attempts advisory here; the hard suppression happened at step 0.
    out.push_str(&super::place::refused_block(store, &tokens));
    out.push_str(&super::place::prior_attempts_block(store, &tokens));

    // 4) Dependency fan-out to expect (ring 1 + knock-on), per similar module.
    out.push_str("dependencies to expect:\n");
    let mut views = 0;
    for (path, _) in &similar {
        if views >= MAX_DEPENDENTS_VIEWS {
            break;
        }
        match crate::search::dependents::build(store, path) {
            Ok(report) if !report.direct.is_empty() || !report.ring_two.is_empty() => {
                views += 1;
                for line in report.render().lines().take(6) {
                    out.push_str(&format!("  {line}\n"));
                }
            }
            Ok(_) => {} // leaf: nothing imports it; a quiet section says enough
            Err(_) => out.push_str(&format!("  ! no import data for {path}\n")),
        }
    }
    if views == 0 {
        out.push_str("  (no registered dependents on the matched modules — leaf changes)\n");
    }

    // 5) Test expectations: what similar components register, plus the rule.
    out.push_str(&format!("test expectations:\n{TEST_OBLIGATION}\n"));
    for (path, _) in &similar {
        if let Some(comp) = find_registered(&manifest, path) {
            if !comp.tests.is_empty() {
                out.push_str(&format!(
                    "  - component `{}` registers tests: {} — same standard for a change here\n",
                    comp.name,
                    comp.tests.join(", ")
                ));
            }
        }
    }

    // 6) Conventions and the commit-time record.
    out.push_str(&format!(
        "{DATA_CONVENTION}\n{ADD_PRINCIPLE}\n{BORN_MODULAR}\n"
    ));
    out.push_str(
        "record: when this plan becomes a decision, log it \
         (`harna log-decision --title .. --body .. [--supersedes old.md]`) and keep .harna/ in \
         the same commit as the code\n",
    );

    Ok(out)
}

/// Find a registered component (including children) whose path matches.
fn find_registered(
    manifest: &crate::schema::model::manifest::Manifest,
    path: &str,
) -> Option<crate::schema::model::manifest::Component> {
    let wanted = path.replace('\\', "/");
    for l in &manifest.layers {
        for c in &l.containers {
            let mut stack: Vec<&crate::schema::model::manifest::Component> =
                c.components.iter().collect();
            while let Some(comp) = stack.pop() {
                if comp.path == wanted {
                    return Some(comp.clone());
                }
                stack.extend(comp.children.iter());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::digest::{Digest, Module};

    fn fixture_store() -> Store {
        // Thread-name tagging (same recipe as scaffold): two parallel plan
        // tests on one temp repo would delete each other's .harna mid-run.
        let dir = std::env::temp_dir().join(format!(
            "harna-plan-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("plan").replace("::", "__")
        ));
        let _ = std::fs::remove_dir_all(dir.join(".harna"));
        let store = Store::open(&dir).unwrap();
        store.init().unwrap();

        std::fs::write(dir.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/t\"]\n").unwrap();
        std::fs::create_dir_all(dir.join("crates/t")).unwrap();
        std::fs::write(dir.join("crates/t/Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();

        std::fs::write(
            store.manifest_path(),
            r#"
schema_version = 1

[[layers]]
name = "memory"

[[layers.containers]]
name = "schema"
path = "crates/t/src/schema"
description = "data model"

[[layers.containers.components]]
name = "store"
kind = "primitive"
path = "crates/t/src/schema/store.rs"
summary = "persistence"
tests = ["crates/t/src/schema/store.rs"]
"#,
        )
        .unwrap();

        std::fs::write(
            store.rules_path(),
            r#"
schema_version = 1

[[rules]]
id = "constants-are-data"
text = "Value-set literals live in data files, imported once"
rationale = "One source beats four copies"
scope = ["crates/t"]
origin = "test"
check = "tests-declared"
"#,
        )
        .unwrap();

        // Live decision + one visibly dead it supersedes.
        {
            let dir_d = store.decisions_dir();
            std::fs::create_dir_all(&dir_d).unwrap();
            std::fs::write(
                dir_d.join("2026-01-01-old-store.md"),
                crate::schema::journal::decisions::markdown("Old store plan", "crypto", None),
            )
            .unwrap();
            std::fs::write(
                dir_d.join("2026-01-02-new-store.md"),
                crate::schema::journal::decisions::markdown(
                    "New store plan",
                    "current",
                    Some("2026-01-01-old-store.md"),
                ),
            )
            .unwrap();
        }

        // Two matching modules; the first imports an engine module.
        store
            .save_digest(
                &store.load_manifest().unwrap_or_default(),
                &Digest {
                generated_at: "now".into(),
                extractor_version: crate::search::digest::EXTRACTOR_VERSION,
                modules: vec![
                    Module {
                        path: "crates/t/src/schema/store.rs".into(),
                        lang: "rust".into(),
                        signatures: vec!["pub struct StoreSchema".into()],
                        deps: vec![],
                        body_hash: "x".into(),
                        inventory: vec![crate::search::digest::Item {
                            vis: "pub".into(),
                            kind: "enum".into(),
                            name: "LedgerEvents".into(),
                            text: "pub enum LedgerEvents".into(),
                        }],
                        mods: vec![],
                    },
                    Module {
                        path: "crates/t/src/api.rs".into(),
                        lang: "rust".into(),
                        signatures: vec!["pub fn call()".into()],
                        deps: vec!["t::schema::store::StoreSchema".into()],
                        body_hash: "x".into(),
                        inventory: vec![],
                        mods: vec![],
                    },
                ],
            })
            .unwrap();
        store
    }

    #[test]
    fn briefing_is_a_decision_proposal() {
        let store = fixture_store();
        let text = build(&store, "store schema plan").unwrap();

        // Placement with candidate containers.
        assert!(text.contains("crates/t/src/schema/store.rs"));
        assert!(text.contains("[layer memory, container schema]"));
        assert!(text.contains("candidates to extend: schema (layer memory)"));
        // Decision context: live pointer, dead predecessor marked.
        assert!(text.contains("New store plan — supersedes 2026-01-01-old-store.md"));
        assert!(text.contains("Old store plan"));
        assert!(
            text.contains("Old store plan — VISIBLY DEAD: superseded by 2026-01-02-new-store.md"),
            "briefing was:\n{text}"
        );
        // Rule with its bound check id.
        assert!(text.contains("constants-are-data: Value-set literals live in data files"));
        assert!(text.contains("[check: tests-declared]"));
        // Born-modular guidance rides the plan (0.3.26).
        assert!(text.contains("born modular"), "briefing was:\n{text}");
        // Fan-out preview names the importer.
        assert!(text.contains("crates/t/src/api.rs"));
        // Test expectation: what the matched component registers.
        assert!(text.contains("tests: crates/t/src/schema/store.rs"));
        assert!(text.contains("no-test-update"));
        // Conventions and the record reminder.
        assert!(text.contains("data files"));
        assert!(text.contains("inside that engine's own container"));
        assert!(text.contains("log-decision"));
    }

    #[test]
    fn generic_queries_are_rejected() {
        let store = fixture_store();
        assert!(build(&store, "the of and").is_err());
    }

    #[test]
    fn a_refused_idea_is_suppressed_to_the_supersession_path() {
        let store = fixture_store();
        let mut g = crate::schema::registers::non_goals::NonGoals::default();
        g.upsert(crate::schema::registers::non_goals::NonGoal {
            id: "no-native-store".into(),
            refusal: "no native store engine in this app".into(),
            reason: "the hosted store owns the schema; two sources of truth were the 0.1 failure".into(),
            recorded: "2026-10-17".into(),
        });
        store.save_non_goals(&g).unwrap();

        // The FULL token set of the idea sits inside the refusal: suppressed.
        let text = build(&store, "native store schema").unwrap();
        assert!(text.contains("REFUSED: an active non-goal covers this whole idea"));
        assert!(text.contains("[no-native-store]"));
        assert!(text.contains("harna non-goal remove <id>"));
        // Suppression means suppression: no placement, no candidates.
        assert!(!text.contains("candidates to extend"), "was:\n{text}");
        assert!(!text.contains("test expectations"));

        // A sibling idea sharing ONE token with the refusal is advised,
        // not suppressed: the briefing proceeds and merely names the block.
        let text2 = build(&store, "store schema plan").unwrap();
        assert!(!text2.contains("REFUSED: an active non-goal covers"));
        assert!(text2.contains("candidates to extend: schema (layer memory)"));
    }

    #[test]
    fn prior_attempts_are_mined_into_the_plan() {
        let store = fixture_store();
        let mut l = crate::schema::registers::lessons::Lessons::default();
        l.upsert(crate::schema::registers::lessons::Lesson {
            id: "hand-rolled-ledger".into(),
            claim: "hand-roll the store schema as a flat ledger".into(),
            verdict: "failed: no joins across event boundaries, query-side pain".into(),
            evidence: vec!["commit d4e81ed".into()],
            recorded: "2026-10-17".into(),
        });
        store.save_lessons(&l).unwrap();

        let text = build(&store, "store schema plan").unwrap();
        assert!(text.contains("prior attempts (recorded dead ends"));
        assert!(text.contains("[hand-rolled-ledger]"));
        assert!(text.contains("commit d4e81ed"));
        assert!(
            text.contains("store schema"),
            "the claim carries the idea's tokens, briefing:\n{text}"
        );
    }

    #[test]
    fn matching_is_inventory_overlap_not_signature_substring() {
        let store = fixture_store();
        // `LedgerEvents` lives only in the inventory — no signature spells it.
        let text = build(&store, "ledger events plan").unwrap();
        assert!(
            text.contains("crates/t/src/schema/store.rs"),
            "an inventory item name must match, briefing was:\n{text}"
        );
    }

    #[test]
    fn unknown_idea_reports_new_placement() {
        let store = fixture_store();
        let text = build(&store, "quantum flux capacitor").unwrap();
        assert!(text.contains("similar: none found"));
        // Still a complete briefing: obligations named even with no matches.
        assert!(text.contains("no-test-update"));
        assert!(text.contains("decision context: none matched"));
    }
}
