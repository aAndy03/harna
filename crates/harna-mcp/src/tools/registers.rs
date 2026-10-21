//! MCP tools for the declared-data registers the CLI already wrote:
//! non-goals, lessons, roadmap, glossary. Each takes an `action`
//! (add | remove | list, plus `set` for roadmap states).

use std::future::Future;

use rmcp::{handler::server::tool::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{required, today, HarnaTools};

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct NonGoalRequest {
    /// "add" | "remove" | "list".
    pub action: String,
    /// Stable id, e.g. "no-remote-state" (add/remove).
    pub id: Option<String>,
    /// What is refused, imperative (add).
    pub refusal: Option<String>,
    /// Why it was refused (add).
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct LessonRequest {
    /// "add" | "remove" | "list".
    pub action: String,
    /// Stable id (add/remove).
    pub id: Option<String>,
    /// What was tried (add).
    pub claim: Option<String>,
    /// Why it failed (add).
    pub verdict: Option<String>,
    /// Evidence pointers: commits, files, issues (add, optional).
    pub evidence: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PluginRequest {
    /// list | show | add | remove
    pub action: String,
    /// Plugin name or folder path.
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct QualityRequest {
    /// "add" | "remove" | "list".
    pub action: String,
    pub id: Option<String>,
    pub statement: Option<String>,
    pub measure: Option<String>,
    pub category: Option<String>,
    pub scope: Option<Vec<String>>,
    pub why: Option<String>,
    pub held_by: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ServiceRequest {
    pub action: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub containers: Option<Vec<String>>,
    pub owns: Option<Vec<String>>,
    pub owners: Option<Vec<String>>,
    pub runs: Option<String>,
    pub kind: Option<String>,
    pub from: Option<Vec<String>>,
    pub to: Option<Vec<String>>,
    pub messages: Option<Vec<String>>,
    /// corridor: messages declared before their code exists.
    pub planned: Option<bool>,
    pub facet: Option<String>,
    pub answer: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CommandRequest {
    /// add | remove | list
    pub action: String,
    pub name: Option<String>,
    /// The exact invocation.
    pub run: Option<String>,
    /// What a pass proves.
    pub proves: Option<String>,
    /// local | ci | both
    pub runs: Option<String>,
    pub scope: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RoadmapRequest {
    /// "add" | "set" | "remove" | "list".
    pub action: String,
    /// Milestone id, e.g. "0.3.33" (add/set/remove).
    pub id: Option<String>,
    /// What it delivers, one line (add).
    pub title: Option<String>,
    /// declared | current | done | dropped (add defaults to declared; set requires it).
    pub state: Option<String>,
    /// Optional longer goal (add).
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct TermsRequest {
    /// "add" | "remove" | "list".
    pub action: String,
    /// harna's canonical word (add/remove).
    pub word: Option<String>,
    /// Its meaning, as harna defines it (add).
    pub means: Option<String>,
    /// Agent vocabulary that maps to it (add, optional).
    pub aliases: Option<Vec<String>>,
}

fn check_state(state: &str) -> anyhow::Result<()> {
    if !harna_core::schema::registers::roadmap::Milestone::STATES.contains(&state) {
        anyhow::bail!("unknown state `{state}` — one of declared|current|done|dropped");
    }
    Ok(())
}

fn unknown(action: &str, allowed: &str) -> anyhow::Error {
    anyhow::anyhow!("unknown action `{action}` — {allowed}")
}

#[tool_router(router = registers_router, vis = "pub(super)")]
impl HarnaTools {
    #[tool(
        description = "Product refusals: 'add' (id, refusal, reason; plan/place suppress matching ideas), 'remove', 'list'."
    )]
    async fn harna_non_goal(
        &self,
        Parameters(NonGoalRequest {
            action,
            id,
            refusal,
            reason,
        }): Parameters<NonGoalRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut g = store.load_non_goals()?;
            match action.as_str() {
                "add" => {
                    let id = required(id, "id", "add")?;
                    g.upsert(harna_core::schema::registers::non_goals::NonGoal {
                        id: id.clone(),
                        refusal: required(refusal, "refusal", "add")?,
                        reason: required(reason, "reason", "add")?,
                        recorded: today(),
                    });
                    store.save_non_goals(&g)?;
                    Ok(format!("refusal `{id}` in force ({})", g.goals.len()))
                }
                "remove" => {
                    let id = required(id, "id", "remove")?;
                    if !g.remove(&id) {
                        anyhow::bail!("no refusal `{id}` recorded");
                    }
                    store.save_non_goals(&g)?;
                    Ok(format!(
                        "refusal `{id}` removed — log WHY with harna_log_decision"
                    ))
                }
                "list" if g.goals.is_empty() => Ok("no refusals recorded".into()),
                "list" => Ok(g
                    .goals
                    .iter()
                    .map(|e| format!("- [{}] {} — why: {}", e.id, e.refusal, e.reason))
                    .collect::<Vec<_>>()
                    .join("\n")),
                other => Err(unknown(other, "add | remove | list")),
            }
        })
    }

    #[tool(
        description = "The repo's commands: 'add' (name, run, proves, runs local|ci|both, scope), 'remove', 'list'."
    )]
    async fn harna_command(
        &self,
        Parameters(CommandRequest {
            action,
            name,
            run,
            proves,
            runs,
            scope,
        }): Parameters<CommandRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut c = store.load_commands()?;
            match action.as_str() {
                "add" => {
                    let name = required(name, "name", "add")?;
                    let runs = runs.unwrap_or_else(|| "both".into());
                    if !["local", "ci", "both"].contains(&runs.as_str()) {
                        anyhow::bail!("runs is one of local | ci | both");
                    }
                    c.upsert(harna_core::schema::registers::commands::Command {
                        name: name.clone(),
                        run: required(run, "run", "add")?,
                        proves: required(proves, "proves", "add")?,
                        runs,
                        scope: scope.unwrap_or_default(),
                    });
                    store.save_commands(&c)?;
                    Ok(format!("command `{name}` recorded ({})", c.commands.len()))
                }
                "remove" => {
                    let name = required(name, "name", "remove")?;
                    if !c.remove(&name) {
                        anyhow::bail!("no command `{name}` registered");
                    }
                    store.save_commands(&c)?;
                    Ok(format!("command `{name}` removed"))
                }
                "list" if c.is_empty() => Ok("no commands registered".into()),
                "list" => Ok(c
                    .commands
                    .iter()
                    .map(|x| format!("{} [{}]: {} — proves {}", x.name, x.runs, x.run, x.proves))
                    .collect::<Vec<_>>()
                    .join("\n")),
                other => Err(unknown(other, "add | remove | list")),
            }
        })
    }

    /// Plugins (0.5.0): framework knowledge as versioned packs.
    #[tool(
        description = "Framework plugins (gpui-kit 0.8.0, …): list, show, add (install/upgrade into .harna/plugins), remove."
    )]
    async fn harna_plugin(
        &self,
        Parameters(PluginRequest { action, name }): Parameters<PluginRequest>,
    ) -> Result<String, String> {
        use harna_core::generation::structure::plugin;
        self.call(move |store| {
            let home = plugin::home_dir();
            match action.as_str() {
                "list" => {
                    let installed = store.load_plugins()?;
                    let mut out: Vec<String> = installed
                        .iter()
                        .map(|p| format!("installed: {} {}", p.meta.name, p.meta.version))
                        .collect();
                    for p in plugin::available(home.as_deref()) {
                        if !installed.iter().any(|i| i.meta.name == p.meta.name) {
                            out.push(format!(
                                "available: {} {} — {}",
                                p.meta.name, p.meta.version, p.meta.description
                            ));
                        }
                    }
                    Ok(out.join("\n"))
                }
                "show" => {
                    let n = required(name, "name", "show")?;
                    Ok(plugin::describe(&plugin::resolve(&n, home.as_deref())?))
                }
                "add" => {
                    let n = required(name, "name", "add")?;
                    let pack = plugin::resolve(&n, home.as_deref())?;
                    harna_core::enforcement::admission::admit_pack(&pack.rules())?;
                    store.init()?;
                    Ok(plugin::add(&store, &pack)?.join("\n"))
                }
                "remove" => plugin::remove(&store, &required(name, "name", "remove")?),
                other => Err(unknown(other, "list | show | add | remove")),
            }
        })
    }

    #[tool(
        description = "Services: add/set/remove/list; corridor/uncorridor (from, to, messages Name=file::Type); require/ready (facet, answer)."
    )]
    async fn harna_service(
        &self,
        Parameters(ServiceRequest {
            action,
            name,
            description,
            containers,
            owns,
            owners,
            runs,
            kind,
            from,
            to,
            messages,
            planned,
            facet,
            answer,
        }): Parameters<ServiceRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            // the one core the CLI uses: same cleaning, same refusals (0.8.4)
            use harna_core::generation::structure::edit::services as edit;
            use harna_core::schema::store::services::{Corridor, Message};
            let mut s = store.load_services()?;
            let manifest = store.load_manifest()?;
            let out = match action.as_str() {
                "add" | "set" => edit::put_service(
                    &mut s,
                    &manifest,
                    &required(name, "name", &action)?,
                    edit::ServiceEdit {
                        description,
                        containers,
                        owns,
                        owners,
                        runs,
                    },
                    action == "add",
                )?,
                "remove" => edit::remove_service(&mut s, &required(name, "name", "remove")?)?,
                "corridor" => {
                    let c = Corridor {
                        name: required(name, "name", "corridor")?,
                        kind: kind.unwrap_or_default(),
                        from: from.unwrap_or_default(),
                        to: to.unwrap_or_default(),
                        messages: messages
                            .unwrap_or_default()
                            .iter()
                            .map(|m| {
                                Message::parse(m).map(|m| Message {
                                    planned: planned.unwrap_or(false),
                                    ..m
                                })
                            })
                            .collect::<anyhow::Result<_>>()?,
                    };
                    edit::put_corridor(&mut s, &store.repo_root(), c)?
                }
                "uncorridor" => {
                    edit::remove_corridor(&mut s, &required(name, "name", "uncorridor")?)?
                }
                "require" => edit::set_require(&mut s, facet.map(|f| vec![f]))?,
                "ready" => edit::set_ready(
                    &mut s,
                    &required(name, "name", "ready")?,
                    &required(facet, "facet", "ready")?,
                    &answer.unwrap_or_default(),
                )?,
                "list" if s.is_empty() => return Ok("no services declared".into()),
                "list" => {
                    let mut lines = s.lines();
                    let unready = s.unready();
                    if !unready.is_empty() {
                        lines.push(format!(
                            "readiness missing: {}",
                            unready
                                .iter()
                                .map(|(a, b)| format!("{a}/{b}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    return Ok(lines.join("\n"));
                }
                other => {
                    return Err(unknown(
                        other,
                        "add | set | remove | corridor | uncorridor | require | ready | list",
                    ))
                }
            };
            store.save_services(&s)?;
            Ok(out)
        })
    }

    #[tool(
        description = "Long-term qualities (scale, latency, offline, privacy): add, remove, list."
    )]
    async fn harna_quality(
        &self,
        Parameters(QualityRequest {
            action,
            id,
            statement,
            measure,
            category,
            scope,
            why,
            held_by,
        }): Parameters<QualityRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut q = store.load_qualities()?;
            match action.as_str() {
                "add" => {
                    let id = required(id, "id", "add")?;
                    let held_by = held_by.unwrap_or_default();
                    let rules = if held_by.is_empty() {
                        Default::default()
                    } else {
                        store.load_effective_rules()?
                    };
                    if let Some(r) =
                        harna_core::schema::registers::qualities::unknown_holder(&held_by, &rules)
                    {
                        anyhow::bail!("held_by `{r}` names no rule");
                    }
                    q.upsert(harna_core::schema::registers::qualities::Quality {
                        id: id.clone(),
                        statement: required(statement, "statement", "add")?,
                        measure: measure.unwrap_or_default(),
                        category: category.unwrap_or_default(),
                        scope: scope.unwrap_or_default(),
                        why: why.unwrap_or_default(),
                        recorded: today(),
                        held_by,
                    })?;
                    store.save_qualities(&q)?;
                    Ok(format!("quality `{id}` recorded ({})", q.qualities.len()))
                }
                "remove" => {
                    let id = required(id, "id", "remove")?;
                    if !q.remove(&id) {
                        anyhow::bail!("no quality `{id}` declared");
                    }
                    store.save_qualities(&q)?;
                    Ok(format!("quality `{id}` removed"))
                }
                "list" if q.is_empty() => Ok("no qualities declared".into()),
                "list" => Ok(q
                    .qualities
                    .iter()
                    .map(|e| format!("- {}", e.line()))
                    .collect::<Vec<_>>()
                    .join("\n")),
                other => Err(unknown(other, "add | remove | list")),
            }
        })
    }

    #[tool(
        description = "Dead ends: 'add' (id, claim, verdict, evidence) so plan/place warn before re-suggesting, 'remove', 'list'."
    )]
    async fn harna_lesson(
        &self,
        Parameters(LessonRequest {
            action,
            id,
            claim,
            verdict,
            evidence,
        }): Parameters<LessonRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut l = store.load_lessons()?;
            match action.as_str() {
                "add" => {
                    let id = required(id, "id", "add")?;
                    l.upsert(harna_core::schema::registers::lessons::Lesson {
                        id: id.clone(),
                        claim: required(claim, "claim", "add")?,
                        verdict: required(verdict, "verdict", "add")?,
                        evidence: evidence.unwrap_or_default(),
                        recorded: today(),
                    });
                    store.save_lessons(&l)?;
                    Ok(format!("lesson `{id}` recorded ({})", l.lessons.len()))
                }
                "remove" => {
                    let id = required(id, "id", "remove")?;
                    if !l.remove(&id) {
                        anyhow::bail!("no lesson `{id}` recorded");
                    }
                    store.save_lessons(&l)?;
                    Ok(format!("lesson `{id}` removed"))
                }
                "list" if l.lessons.is_empty() => Ok("no dead ends recorded".into()),
                "list" => Ok(l
                    .lessons
                    .iter()
                    .map(|e| format!("- [{}] {} — verdict: {}", e.id, e.claim, e.verdict))
                    .collect::<Vec<_>>()
                    .join("\n")),
                other => Err(unknown(other, "add | remove | list")),
            }
        })
    }

    #[tool(
        description = "Milestones: 'add' (id, title, state), 'set' (declared|current|done|dropped), 'remove', 'list'."
    )]
    async fn harna_roadmap(
        &self,
        Parameters(RoadmapRequest {
            action,
            id,
            title,
            state,
            notes,
        }): Parameters<RoadmapRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut r = store.load_roadmap()?;
            match action.as_str() {
                "add" => {
                    let id = required(id, "id", "add")?;
                    let state = state.unwrap_or_else(|| "declared".into());
                    check_state(&state)?;
                    r.upsert(harna_core::schema::registers::roadmap::Milestone {
                        id: id.clone(),
                        title: required(title, "title", "add")?,
                        state,
                        notes: notes.unwrap_or_default(),
                    });
                    store.save_roadmap(&r)?;
                    Ok(format!(
                        "milestone `{id}` declared ({})",
                        r.milestones.len()
                    ))
                }
                "set" => {
                    let id = required(id, "id", "set")?;
                    let state = required(state, "state", "set")?;
                    check_state(&state)?;
                    let m = r
                        .milestones
                        .iter_mut()
                        .find(|m| m.id.eq_ignore_ascii_case(&id))
                        .ok_or_else(|| anyhow::anyhow!("no milestone `{id}` declared"))?;
                    m.state = state.clone();
                    store.save_roadmap(&r)?;
                    Ok(format!("milestone `{id}` is now `{state}`"))
                }
                "remove" => {
                    let id = required(id, "id", "remove")?;
                    if !r.remove(&id) {
                        anyhow::bail!("no milestone `{id}` declared");
                    }
                    store.save_roadmap(&r)?;
                    Ok(format!("milestone `{id}` removed — log why"))
                }
                "list" if r.milestones.is_empty() => Ok("no milestones declared".into()),
                "list" => Ok(r
                    .milestones
                    .iter()
                    .map(|m| format!("[{}] {} = {}", m.state, m.id, m.title))
                    .collect::<Vec<_>>()
                    .join("\n")),
                other => Err(unknown(other, "add | set | remove | list")),
            }
        })
    }

    #[tool(
        description = "Glossary: 'add' (word, means, aliases; warns on collisions), 'remove', 'list'."
    )]
    async fn harna_terms(
        &self,
        Parameters(TermsRequest {
            action,
            word,
            means,
            aliases,
        }): Parameters<TermsRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut t = store.load_terms()?;
            match action.as_str() {
                "add" => {
                    let word = required(word, "word", "add")?;
                    t.terms.retain(|e| !e.word.eq_ignore_ascii_case(&word));
                    t.terms.push(harna_core::schema::registers::terms::Term {
                        word: word.clone(),
                        aliases: aliases.unwrap_or_default(),
                        means: required(means, "means", "add")?,
                    });
                    let collisions = t.alias_collisions();
                    store.save_terms(&t)?;
                    let mut out = format!("term `{word}` recorded ({} total)", t.terms.len());
                    if !collisions.is_empty() {
                        out.push_str(&format!(
                            "\nWATCH: alias collision(s) remain — {collisions:?}"
                        ));
                    }
                    Ok(out)
                }
                "remove" => {
                    let word = required(word, "word", "remove")?;
                    let before = t.terms.len();
                    t.terms.retain(|e| !e.word.eq_ignore_ascii_case(&word));
                    if t.terms.len() == before {
                        anyhow::bail!("no term `{word}` in the glossary");
                    }
                    store.save_terms(&t)?;
                    Ok(format!("term `{word}` removed"))
                }
                "list" if t.terms.is_empty() => Ok("glossary empty".into()),
                "list" => Ok(t
                    .terms
                    .iter()
                    .map(|e| format!("{} = {}", e.word, e.means))
                    .collect::<Vec<_>>()
                    .join("\n")),
                other => Err(unknown(other, "add | remove | list")),
            }
        })
    }
}
