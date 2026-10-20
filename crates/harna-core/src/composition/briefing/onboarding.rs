//! Onboarding: the ~1,000-token digest a fresh agent session starts with.
//!
//! One call replaces the "re-read everything after compaction" loop.

use crate::schema::model::manifest::Manifest;
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

/// Installed plugins (0.5.9): which framework packs speak here, what they
/// mirror, and the capabilities a container can own.
fn plugins_brief(store: &Store) -> String {
    let (ok, broken) = store.load_plugins_lenient();
    let mut out = String::new();
    for p in &ok {
        let mirrors: Vec<String> = p
            .meta
            .mirrors
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect();
        let caps: Vec<String> = p
            .meta
            .capabilities
            .keys()
            .map(|c| format!("{}:{c}", p.meta.name))
            .collect();
        out.push_str(&format!(
            "plugin {} {} — mirrors {}; {} rule(s) as {}/<id>{}\n",
            p.meta.name,
            p.meta.version,
            if mirrors.is_empty() {
                "nothing".into()
            } else {
                mirrors.join(", ")
            },
            p.rules.rules.len(),
            p.meta.name,
            if caps.is_empty() {
                String::new()
            } else {
                format!("; capabilities a container can own: {}", caps.join(", "))
            }
        ));
    }
    for why in broken {
        out.push_str(&format!("plugin BROKEN (its rules do not apply): {why}\n"));
    }
    out
}

pub fn build(store: &Store) -> anyhow::Result<Onboarding> {
    let manifest = store.load_manifest().unwrap_or_default();
    let rules = store.load_effective_rules().unwrap_or_default();

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
    text.push_str(&roadmap_brief(store));
    text.push_str(&rules.render_for_agent(None, None));
    text.push_str(&super::place::qualities_block(
        store,
        &[],
        "qualities (what the system must hold over its life)",
    ));
    // services as data (0.8.0): what runs on its own, what each owns
    if let Ok(s) = store.load_services() {
        if !s.services.is_empty() {
            text.push_str("services (each runs its containers alone; one owner per resource):\n");
            for l in s.lines() {
                text.push_str(&format!("  {l}\n"));
            }
        }
    }
    text.push_str(&plugins_brief(store));
    text.push_str(&terms_brief(store));
    text.push_str(&registers_brief(store));
    text.push_str(&commands_brief(store));
    // The toolbox in one line (0.3.50): visible from the first message.
    text.push_str(&format!(
        "toolbox: {} optional tools — `harna tools` / harna://tools; place and plan offer the ones that fit\n",
        super::toolbox::catalog().len()
    ));
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
    text.push_str(&crate::schema::journal::handoff::onboarding_text(store));

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
        out.push_str(&format!(
            "layer [{}]{}\n",
            layer.name,
            if layer.owners.is_empty() {
                String::new()
            } else {
                format!(" owners: {}", layer.owners.join(", "))
            }
        ));
        // an empty boot order is the common case, not news (0.4.9)
        if !layer.sequence.is_empty() {
            out.push_str(&format!("  boot: {}\n", layer.sequence.join(" -> ")));
        }
        for c in &layer.containers {
            let seq = if c.sequence.is_empty() {
                String::new()
            } else {
                format!(" [{}]", c.sequence.join(", "))
            };
            let owners = if c.owners.is_empty() {
                String::new()
            } else {
                format!(" owners: {}", c.owners.join(", "))
            };
            out.push_str(&format!("  container {}{}{owners}\n", c.name, seq));
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
    match store.load_effective_terms() {
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
                out.push_str(&format!(
                    "  … +{rest} more terms (read .harna/terms.toml)\n"
                ));
            }
            out
        }
        // No glossary recorded: the built-in concept map stands in, so a
        // fresh repo's agent still learns the vocabulary once, here.
        Ok(_) => concepts_brief(),
        Err(_) => {
            "glossary: .harna/terms.toml is unparseable — fix it (doctor names the damage)\n".into()
        }
    }
}

/// The refusals and dead ends in force (0.3.42, from the font-manager
/// test: an agent only met them through place/plan, after it had already
/// chosen a direction). Bounded like the glossary: the first entries detail,
/// the rest count; long lines clip. A broken register is named, never
/// silently empty — a silent refusal list would read as "nothing refused".
fn registers_brief(store: &Store) -> String {
    const MAX_ENTRIES: usize = 8;
    const MAX_LINE: usize = 140;
    let clip = |s: String| -> String {
        if s.chars().count() <= MAX_LINE {
            s
        } else {
            format!("{}…", s.chars().take(MAX_LINE).collect::<String>())
        }
    };
    let tail = |n: usize, file: &str| {
        if n > MAX_ENTRIES {
            format!("  … +{} more (read .harna/{file})\n", n - MAX_ENTRIES)
        } else {
            String::new()
        }
    };
    let mut out = String::new();
    match store.load_non_goals() {
        Ok(g) if !g.is_empty() => {
            out.push_str(&format!(
                "refused ({} non-goal(s) in force — do not propose; superseding needs the user):\n",
                g.goals.len()
            ));
            for n in g.goals.iter().take(MAX_ENTRIES) {
                out.push_str(&clip(format!(
                    "  [{}] {} — why: {}",
                    n.id, n.refusal, n.reason
                )));
                out.push('\n');
            }
            out.push_str(&tail(g.goals.len(), "non-goals.toml"));
        }
        Ok(_) => {}
        Err(_) => out.push_str(
            "refused: .harna/non-goals.toml is unparseable — do not act unrefused; fix it first\n",
        ),
    }
    match store.load_lessons() {
        Ok(l) if !l.is_empty() => {
            out.push_str(&format!(
                "dead ends ({} lesson(s) — re-trying one needs a reason the verdict no longer holds):\n",
                l.lessons.len()
            ));
            for n in l.lessons.iter().take(MAX_ENTRIES) {
                out.push_str(&clip(format!(
                    "  [{}] {} — verdict: {}",
                    n.id, n.claim, n.verdict
                )));
                out.push('\n');
            }
            out.push_str(&tail(l.lessons.len(), "lessons.toml"));
        }
        Ok(_) => {}
        Err(_) => out.push_str(
            "dead ends: .harna/lessons.toml is unparseable — recorded failures are out of view\n",
        ),
    }
    out
}

/// The repo's own commands (0.3.46): the right invocation and what it
/// proves, before an agent guesses one. Bounded like the other briefs.
fn commands_brief(store: &Store) -> String {
    const MAX: usize = 10;
    match store.load_commands() {
        Ok(c) if !c.is_empty() => {
            let mut out = format!(
                "commands ({} registered — run these, not guesses):\n",
                c.commands.len()
            );
            for x in c.commands.iter().take(MAX) {
                let scope = if x.scope.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", x.scope.join(", "))
                };
                out.push_str(&format!(
                    "  {}: `{}` — proves {}{scope}\n",
                    x.name, x.run, x.proves
                ));
            }
            if c.commands.len() > MAX {
                out.push_str(&format!(
                    "  … +{} more (harna command list)\n",
                    c.commands.len() - MAX
                ));
            }
            out
        }
        Ok(_) => String::new(),
        Err(_) => "commands: .harna/commands.toml is unparseable — fix it\n".into(),
    }
}

/// harna's built-in concept map (`concepts.toml`, data): the meanings tool
/// descriptions no longer repeat. One line per concept.
pub fn concepts() -> crate::schema::registers::terms::Terms {
    toml::from_str(include_str!("concepts.toml")).expect("concepts.toml must be valid data")
}

fn concepts_brief() -> String {
    let mut out = String::from(
        "concepts: harna's vocabulary (built in; record your own in .harna/terms.toml)\n",
    );
    for term in concepts().terms {
        out.push_str(&format!("  {} = {}\n", term.word, term.means));
    }
    out
}

/// The roadmap in one line (0.3.28): the current milestone and where the
/// path goes — a fresh session knows TODAY inside the declared arc, not
/// just what exists (arch overview) and what rules (rules render).
fn roadmap_brief(store: &Store) -> String {
    match store.load_roadmap() {
        Ok(r) if !r.is_empty() => {
            let heading = "roadmap: declared in `.harna/roadmap.toml` (not generated) — this is \
                           where the project is going\n"
                .to_string();
            match r.current() {
                Some(c) => format!("{heading}  NOW: {} — {}\n", c.id, c.title),
                None => String::new(),
            }
        }
        Ok(_) => String::new(),
        Err(_) => {
            "roadmap: .harna/roadmap.toml is unparseable — fix it (doctor names the damage)\n"
                .into()
        }
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
    use crate::schema::model::rules::Rule;
    use crate::schema::model::rules::Rules;

    /// font-manager (0.3.42): the agent recorded `no-font-install` and a
    /// failed `ttf-parser`-only lesson; a fresh session must meet both in
    /// onboarding, before it picks a direction — not only via place/plan.
    #[test]
    fn onboarding_carries_refusals_and_dead_ends_bounded() {
        use crate::schema::registers::lessons::{Lesson, Lessons};
        use crate::schema::registers::non_goals::{NonGoal, NonGoals};
        let tmp = std::env::temp_dir().join(format!(
            "harna-onb-registers-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        assert!(!build(&store).unwrap().text.contains("refused ("));

        let mut g = NonGoals::default();
        g.upsert(NonGoal {
            id: "no-font-install".into(),
            refusal: "the app never installs, deletes or moves font files".into(),
            reason: "read-only manager; system fonts belong to Windows".into(),
            recorded: "2026-10-18".into(),
        });
        store.save_non_goals(&g).unwrap();
        let mut l = Lessons::default();
        for i in 0..10 {
            l.upsert(Lesson {
                id: format!("lesson-{i}"),
                claim: format!("attempt {i} {}", "x".repeat(200)),
                verdict: "did not work".into(),
                evidence: vec![],
                recorded: "2026-10-18".into(),
            });
        }
        store.save_lessons(&l).unwrap();

        let text = build(&store).unwrap().text;
        assert!(text.contains("refused (1 non-goal(s) in force"), "{text}");
        assert!(text.contains("[no-font-install] the app never installs"));
        assert!(text.contains("dead ends (10 lesson(s)"));
        assert!(text.contains("[lesson-7]") && !text.contains("[lesson-8]"));
        assert!(text.contains("… +2 more (read .harna/lessons.toml)"));
        assert!(
            text.lines().all(|line| line.chars().count() <= 160),
            "clipped"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_repo_without_a_glossary_still_meets_the_vocabulary() {
        let tmp = std::env::temp_dir().join(format!(
            "harna-onb-concepts-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let text = build(&store).unwrap().text;
        assert!(
            text.contains("concepts: harna's vocabulary (built in"),
            "{text}"
        );
        for word in [
            "parity",
            "transiter",
            "non-goal",
            "lesson",
            "handoff",
            "acknowledged",
        ] {
            assert!(
                text.contains(&format!("  {word} = ")),
                "missing concept {word}"
            );
        }
        // The built-in map is valid data with one meaning per word.
        let c = concepts();
        assert!(c.terms.len() >= 19);
        assert!(
            c.alias_collisions().is_empty(),
            "{:?}",
            c.alias_collisions()
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn includes_rules_and_flags_empty_manifest() {
        let tmp = std::env::temp_dir().join(format!("harna-onb-{}", std::process::id()));
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        let mut rules = Rules::default();
        rules.rules.push(Rule {
            scope_signature: None,
            command: None,
            values: Vec::new(),
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
        let terms = crate::schema::registers::terms::Terms {
            schema_version: crate::SCHEMA_BASELINE,
            terms: vec![crate::schema::registers::terms::Term {
                word: "engine".into(),
                aliases: vec!["service".into()],
                means: "a role-carrying container hanging off a layer".into(),
            }],
        };
        store.save_terms(&terms).unwrap();
        let onb2 = build(&store).unwrap();
        assert!(onb2.text.contains("glossary:"));
        assert!(onb2
            .text
            .contains("engine (also: service) = a role-carrying"));

        // A recorded handoff arrives through the same call — the resumed
        // session does not need a separate query.
        crate::schema::journal::handoff::save(
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
            reexports: Vec::new(),
            crate_dir: None,
            src_root: None,
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
            orphans: Vec::new(),
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
        assert!(
            onb.text.contains("src/m0.rs"),
            "detail rides the leading modules"
        );
        assert!(
            !onb.text.contains("src/m10.rs"),
            "detail caps before the tail"
        );
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
