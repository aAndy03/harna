//! Onboarding: the digest a fresh agent session starts with.
//!
//! One call replaces the "re-read everything after compaction" loop.
//! Summary first (0.9.5): every session pays for the summary — the
//! architecture, the rules in one line each, what is refused, the commands,
//! where the last burst stopped — and asks for a section when it needs the
//! detail (`harna onboarding rules|glossary|map|registers|conventions|all`).

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

/// The sections a session can ask for after the summary.
pub const SECTIONS: &[&str] = &[
    "rules",
    "glossary",
    "map",
    "registers",
    "conventions",
    "all",
];

/// The summary every session starts with.
pub fn build(store: &Store) -> anyhow::Result<Onboarding> {
    compose(store, false)
}

/// Everything, in full: the pre-0.9.5 briefing.
pub fn build_full(store: &Store) -> anyhow::Result<Onboarding> {
    compose(store, true)
}

/// One section in full, or the summary for an empty name.
pub fn section(store: &Store, name: &str) -> anyhow::Result<String> {
    Ok(match name.trim() {
        "" | "summary" => build(store)?.text,
        "all" => format!(
            "{}{}",
            build_full(store)?.text,
            super::plan::conventions_full()
        ),
        "rules" => store
            .load_effective_rules()
            .unwrap_or_default()
            .render_for_agent(None, None),
        "glossary" => terms_brief(store, usize::MAX),
        "registers" => registers_brief(store, usize::MAX),
        "conventions" => super::plan::conventions_full(),
        "map" => match map_snapshot(store) {
            Some(m) => render_overview_from(
                &m.generated_at,
                m.modules,
                m.signatures,
                &m.first,
                OVERVIEW_MODULES,
            ),
            None => "codebase map: none yet — run `harna scan`\n".into(),
        },
        other => anyhow::bail!(
            "unknown onboarding section `{other}` — one of: {}",
            SECTIONS.join(", ")
        ),
    })
}

/// Bounded map read (0.3.16): counts ride the whole stream, the detail
/// list caps at OVERVIEW_MODULES.
fn map_snapshot(store: &Store) -> Option<MapSnapshot> {
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
        .ok(); // unreadable map = no map
    map
}

fn compose(store: &Store, full: bool) -> anyhow::Result<Onboarding> {
    let manifest = store.load_manifest().unwrap_or_default();
    let rules = store.load_effective_rules().unwrap_or_default();

    let map = map_snapshot(store);

    let mut text = String::new();

    text.push_str(&arch_overview(&manifest));
    text.push_str(&roadmap_brief(store));
    if full {
        text.push_str(&rules.render_for_agent(None, None));
    } else {
        text.push_str(&rules.render_compact());
    }
    text.push_str(&super::place::qualities_block(
        store,
        &[],
        "qualities (what the system must hold over its life)",
    ));
    // services as data (0.8.0): what runs on its own, what each owns
    if let Ok(s) = store.load_services() {
        if !s.is_empty() {
            // the block above may end mid-line (0.8.4, books field test)
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str("services (each runs its containers alone; one owner per resource):\n");
            for l in s.lines() {
                text.push_str(&format!("  {l}\n"));
            }
            let unready = s.unready();
            if !unready.is_empty() {
                text.push_str(&format!(
                    "  ops readiness: {} answer(s) missing — preflight holds the release (harna service show <service>)\n",
                    unready.len()
                ));
            }
        }
    }
    text.push_str(&plugins_brief(store));
    if full {
        text.push_str(&terms_brief(store, 16));
        text.push_str(&registers_brief(store, 8));
    } else {
        text.push_str(&glossary_line(store));
        text.push_str(&registers_brief(store, 0));
    }
    text.push_str(&commands_brief(store));
    // The toolbox in one line (0.3.50): visible from the first message.
    text.push_str(&format!(
        "toolbox: {} optional tools — `harna tools` / harna://tools; place and plan offer the ones that fit\n",
        super::toolbox::catalog().len()
    ));
    if let Some(snap) = &map {
        if full {
            text.push_str(&render_overview_from(
                &snap.generated_at,
                snap.modules,
                snap.signatures,
                &snap.first,
                OVERVIEW_MODULES,
            ));
        } else {
            text.push_str(&format!(
                "codebase map: {} modules, {} signatures (scanned {})\n",
                snap.modules, snap.signatures, snap.generated_at
            ));
        }
    }
    text.push_str(&parity_summary(store));
    // The burst journal: a fresh (or resuming) session first sees where the
    // last burst stopped.
    text.push_str(&crate::schema::journal::handoff::onboarding_text(store));
    if !full {
        text.push_str(
            "more: `harna onboarding <rules|glossary|map|registers|conventions|all>` (MCP: harna_onboarding section) — rules with their why, the whole glossary, the map's modules, every refusal and lesson\n",
        );
    }

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
/// session, so it renders bounded — the first max_terms entries detail,
/// the rest count. A missing file is nothing; an unparseable one is
/// named (a silent glossary would teach the wrong vocabulary).
fn terms_brief(store: &Store, max_terms: usize) -> String {
    match store.load_effective_terms() {
        Ok(t) if !t.is_empty() => {
            let mut out = String::from(
                "glossary: harna's vocabulary (`.harna/terms.toml`) — an agent's own words may \
                 map differently; these are harna's meanings\n",
            );
            for term in t.terms.iter().take(max_terms) {
                let aliases = if term.aliases.is_empty() {
                    String::new()
                } else {
                    format!(" (also: {})", term.aliases.join(", "))
                };
                out.push_str(&format!("  {}{} = {}\n", term.word, aliases, term.means));
            }
            let rest = t.terms.len().saturating_sub(max_terms);
            if rest > 0 {
                out.push_str(&format!(
                    "  … +{rest} more terms (harna onboarding glossary)\n"
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
/// The glossary in one line (0.9.5): the words, not their meanings.
fn glossary_line(store: &Store) -> String {
    let words = |t: &crate::schema::registers::terms::Terms| {
        t.terms
            .iter()
            .map(|x| x.word.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match store.load_effective_terms() {
        Ok(t) if !t.is_empty() => format!(
            "glossary: {} terms with harna's meanings (harna onboarding glossary): {}\n",
            t.terms.len(),
            words(&t)
        ),
        Ok(_) => {
            let c = concepts();
            format!(
                "concepts: {} built-in terms (harna onboarding glossary): {}\n",
                c.terms.len(),
                words(&c)
            )
        }
        Err(_) => terms_brief(store, 0),
    }
}

/// `max_entries` 0 is the summary: the counts and ids, no reasons.
fn registers_brief(store: &Store, max_entries: usize) -> String {
    let ids_only = max_entries == 0;
    let max_entries = if ids_only { 0 } else { max_entries };
    const MAX_LINE: usize = 140;
    let clip = |s: String| -> String {
        if s.chars().count() <= MAX_LINE {
            s
        } else {
            format!("{}…", s.chars().take(MAX_LINE).collect::<String>())
        }
    };
    let tail = |n: usize, file: &str| {
        if max_entries > 0 && n > max_entries {
            format!("  … +{} more (read .harna/{file})\n", n - max_entries)
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
            if ids_only {
                let ids: Vec<&str> = g.goals.iter().map(|n| n.id.as_str()).collect();
                out.push_str(&format!("  {}\n", ids.join(", ")));
            }
            for n in g.goals.iter().take(max_entries) {
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
            if ids_only {
                let ids: Vec<&str> = l.lessons.iter().map(|n| n.id.as_str()).collect();
                out.push_str(&format!("  {}\n", ids.join(", ")));
            }
            for n in l.lessons.iter().take(max_entries) {
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
    toml::from_str(include_str!("../concepts.toml")).expect("concepts.toml must be valid data")
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
mod tests;
