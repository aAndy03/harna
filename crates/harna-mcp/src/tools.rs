//! harna-mcp: an MCP stdio server exposing harna's memory to any agent.
//!
//! Tools: the CLI surface as MCP calls — memory (onboarding, rules,
//! decisions, handoff), map (scan, diff, dependents, whereis, graph),
//! enforcement (validate, parity, deny), generation (scaffold, adopt,
//! render) in `tools.rs`; gates and planners (status, preflight,
//! validate-intent, acknowledge, decompose) in `tools/gates.rs`; the
//! declared registers (non-goal, lesson, roadmap, terms) in
//! `tools/registers.rs`. `tools/list` is the authoritative catalog.
//!
//! Wire into any MCP client by running the `harna-mcp` binary over stdio.

use std::path::PathBuf;

use rmcp::{
    handler::server::router::tool::ToolRouter, handler::server::tool::Parameters, tool,
    tool_handler, tool_router, ServerHandler,
};
use std::future::Future;

use harna_core::composition::briefing::onboarding;
use harna_core::schema::model::rules::Rule;
use harna_core::schema::store::Store;
use harna_core::search::digest;

mod flow;
mod gates;
mod memory;
mod registers;
mod requests;
mod resources;
mod surface;
pub use requests::*;

/// A required argument of an action-dispatched tool (`action = "add"` …).
fn required(v: Option<String>, what: &str, action: &str) -> anyhow::Result<String> {
    v.filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("`{what}` is required for action `{action}`"))
}

#[derive(Debug, Clone)]
pub struct HarnaTools {
    root: PathBuf,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl HarnaTools {
    pub fn new() -> anyhow::Result<Self> {
        // The repo root defaults to the launch directory; override with HARNA_ROOT.
        let root = match std::env::var("HARNA_ROOT") {
            Ok(v) => PathBuf::from(v),
            Err(_) => std::env::current_dir()?,
        };
        Ok(Self {
            root,
            tool_router: surface::shape(
                Self::tool_router()
                    + Self::memory_router()
                    + Self::flow_router()
                    + Self::gates_router()
                    + Self::registers_router(),
                surface::selection(std::env::var("HARNA_TOOLS").ok().as_deref()),
            ),
        })
    }

    fn store(&self) -> anyhow::Result<Store> {
        Store::open(&self.root)
    }

    /// The onboarding digest a fresh agent session should start with.
    #[tool(
        description = "Session start: a summary (architecture, rules, refusals, commands, handoff). Call first; section= for detail."
    )]
    async fn harna_onboarding(
        &self,
        Parameters(OnboardingRequest { section }): Parameters<OnboardingRequest>,
    ) -> Result<String, String> {
        self.call(move |store| onboarding::section(&store, section.as_deref().unwrap_or("")))
    }
    /// Rebuild the compressed code map; agents query this instead of rescanning.
    #[tool(
        description = "Rebuild the code map (.harna/digest/), incremental unless full=true. Run after structural changes."
    )]
    async fn harna_scan(
        &self,
        Parameters(ScanRequest { max_files, full }): Parameters<ScanRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let r = harna_core::composition::scan::run(
                &store,
                &self.root,
                max_files.map_or(harna_core::search::digest::MAX_FILES, |m| m as usize),
                full.unwrap_or(false),
            )?;
            Ok(r.render())
        })
    }

    /// Check the codebase for conformance violations.
    #[tool(
        description = "Conformance check, verdict PASS/BLOCKED. changed_only after edits; strict makes infos block; env scopes rules."
    )]
    async fn harna_validate(
        &self,
        Parameters(ValidateRequest {
            max_files,
            changed_only,
            env,
            strict,
            check,
        }): Parameters<ValidateRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let files = if changed_only.unwrap_or(false) {
                Some(harna_core::enforcement::validate::changed_rust_files(
                    &self.root,
                )?)
            } else {
                None
            };
            let violations = harna_core::enforcement::validate::run_on(
                &self.root,
                &store,
                max_files.map_or(harna_core::search::digest::MAX_FILES, |m| m as usize),
                files,
                env.as_deref(),
            )?;
            // The same gate as the CLI: acknowledged advisories ride as
            // receipts, and the verdict line says whether anything blocks.
            let report = harna_core::enforcement::validate::acknowledge::gate_for(
                &store,
                violations,
                strict.unwrap_or(false),
            );
            // changed_only reads the changed files live (0.9.1)
            let note = if changed_only.unwrap_or(false) {
                None
            } else {
                store
                    .load_digest()
                    .ok()
                    .and_then(|d| harna_core::composition::affected::map_note(&self.root, &d))
            };
            let text = report.render_for(check.as_deref());
            Ok(match note {
                Some(n) => format!("warning: {n}\n{text}"),
                None => text,
            })
        })
    }

    /// Upgrade .harna/ data files to this tool's schema version.
    #[tool(
        description = "Upgrade .harna/ data to this harna's schema (backups first, logged). dry=true previews."
    )]
    async fn harna_migrate(
        &self,
        Parameters(MigrateRequest { dry }): Parameters<MigrateRequest>,
    ) -> Result<String, String> {
        let dry = dry.unwrap_or(false);
        self.call(move |store| {
            let reports = harna_core::schema::store::migrate::run(&self.root, &store, dry)?;
            Ok(reports
                .iter()
                .map(|r| r.render())
                .collect::<Vec<_>>()
                .join("\n"))
        })
    }

    /// Restore the newest backup of a data file.
    #[tool(
        description = "Restore the newest verified backup of 'manifest.toml' or 'rules.toml' over the live file."
    )]
    async fn harna_restore(
        &self,
        Parameters(RestoreRequest { name }): Parameters<RestoreRequest>,
    ) -> Result<String, String> {
        self.call(move |store| harna_core::schema::store::migrate::restore(&store, &name))
    }

    /// Archive closed parity events, superseded decisions and old score snapshots.
    #[tool(
        description = "Archive closed parity events, superseded decisions, old scores (keep newest N). Deletes nothing."
    )]
    async fn harna_archive(
        &self,
        Parameters(ArchiveRequest { keep }): Parameters<ArchiveRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let closed = harna_core::enforcement::parity::archive_closed(&store)?;
            let superseded = harna_core::schema::journal::decisions::archive_superseded(
                &store.decisions_dir(),
                &store.archive_dir(),
            )?;
            let old = harna_core::composition::gates::score::archive_old_snapshots(
                &store,
                keep.unwrap_or(harna_core::composition::gates::score::SNAPSHOT_RETENTION),
            )?;
            Ok(format!(
                "archived: {} closed parity event(s), {} superseded decision(s), {} old score snapshot(s)",
                closed.len(),
                superseded.len(),
                old.len()
            ))
        })
    }

    /// Export the dependency graph (DOT or Mermaid).
    #[tool(
        description = "Dependency graph of the code map, clustered by layer/container. format = 'mermaid' (default) or 'dot'."
    )]
    async fn harna_graph(
        &self,
        Parameters(GraphRequest { format }): Parameters<GraphRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let digest = store.load_digest()?;
            let manifest = store.load_manifest()?;
            let view = harna_core::generation::views::graph::build(&self.root, &manifest, &digest);
            Ok(match format.as_deref().unwrap_or("mermaid") {
                "dot" => {
                    format!(
                        "{} nodes · {} edges\n{}",
                        view.nodes.len(),
                        view.edges.len(),
                        harna_core::generation::views::graph::dot(&view)
                    )
                }
                "mermaid" => {
                    format!(
                        "{} nodes · {} edges\n{}",
                        view.nodes.len(),
                        view.edges.len(),
                        harna_core::generation::views::graph::mermaid(&view)
                    )
                }
                other => anyhow::bail!("unknown graph format `{other}` — use dot or mermaid"),
            })
        })
    }

    /// Parity templates as one tool (0.9.5): list, add.
    #[tool(
        description = "Parity templates (a change type's fan-out ceiling over a scope). action: list (default) | add (id, max_targets, scope, rationale; same id replaces)."
    )]
    async fn harna_template(
        &self,
        Parameters(TemplateRequest {
            action,
            id,
            scope,
            max_targets,
            rationale,
        }): Parameters<TemplateRequest>,
    ) -> Result<String, String> {
        match action.as_deref().unwrap_or("list") {
            "list" => self.template_list().await,
            "add" => {
                let (Some(id), Some(max_targets)) = (id, max_targets) else {
                    return Err("add needs id and max_targets".into());
                };
                self.template_add(Parameters(TemplateAddRequest {
                    id,
                    scope,
                    max_targets,
                    rationale,
                }))
                .await
            }
            other => Err(format!("unknown template action `{other}` — list | add")),
        }
    }

    /// Declare a parity fan-out template.
    async fn template_add(
        &self,
        Parameters(TemplateAddRequest {
            id,
            scope,
            max_targets,
            rationale,
        }): Parameters<TemplateAddRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut t = store.load_templates()?;
            t.templates.retain(|x| x.id != id);
            t.templates.push(
                harna_core::schema::registers::parity_templates::ParityTemplate {
                    id: id.clone(),
                    scope: scope.unwrap_or_default(),
                    max_targets,
                    rationale: rationale.unwrap_or_default(),
                },
            );
            t.templates.sort_by(|a, b| a.id.cmp(&b.id));
            store.save_templates(&t)?;
            Ok(format!("template `{id}` declared — ceiling {max_targets}"))
        })
    }

    /// List the declared parity templates.
    async fn template_list(&self) -> Result<String, String> {
        self.call(|store| {
            let t = store.load_templates()?;
            if t.templates.is_empty() {
                return Ok("no templates declared".to_string());
            }
            Ok(t.templates
                .iter()
                .map(|x| {
                    format!(
                        "- [{}] scope `{}` ceilings {} target(s){}",
                        x.id,
                        if x.scope.is_empty() {
                            "*".to_string()
                        } else {
                            x.scope.clone()
                        },
                        x.max_targets,
                        if x.rationale.is_empty() {
                            String::new()
                        } else {
                            format!(" — why: {}", x.rationale)
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        })
    }

    /// Render a collaboration note draft.
    #[tool(
        description = "Draft notes: kind='review' (parity id or diff ref) or 'release' (since from)."
    )]
    async fn harna_notes(
        &self,
        Parameters(NotesRequest { kind, id, from }): Parameters<NotesRequest>,
    ) -> Result<String, String> {
        self.call(move |store| match kind.as_str() {
            "review" => {
                if id.is_none() && from.is_none() {
                    anyhow::bail!("give a parity event id, or from=<ref> to review a range");
                }
                harna_core::composition::briefing::notes::review(
                    &store,
                    id.as_deref(),
                    from.as_deref(),
                )
            }
            "release" => harna_core::composition::briefing::notes::release(
                &store,
                from.as_deref().unwrap_or("HEAD"),
            ),
            other => anyhow::bail!("unknown notes kind `{other}` — use review or release"),
        })
    }

    /// Deliberate deferrals as one tool (0.9.5): list, add.
    #[tool(
        description = "Deliberate deferrals. action: list (default; overdue flagged) | add (id, component, reason, review = ISO date or version; same id replaces)."
    )]
    async fn harna_deferred(
        &self,
        Parameters(DeferredRequest {
            action,
            id,
            component,
            reason,
            review,
        }): Parameters<DeferredRequest>,
    ) -> Result<String, String> {
        match action.as_deref().unwrap_or("list") {
            "list" => self.deferred_list().await,
            "add" => {
                let (Some(id), Some(component), Some(reason), Some(review)) =
                    (id, component, reason, review)
                else {
                    return Err("add needs id, component, reason and review".into());
                };
                self.deferred_add(Parameters(DeferredAddRequest {
                    id,
                    component,
                    reason,
                    review,
                }))
                .await
            }
            other => Err(format!("unknown deferred action `{other}` — list | add")),
        }
    }

    /// Register a deliberate deferral.
    async fn deferred_add(
        &self,
        Parameters(DeferredAddRequest {
            id,
            component,
            reason,
            review,
        }): Parameters<DeferredAddRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut r = store.load_deferred()?;
            r.upsert(
                harna_core::schema::registers::ledgers::deferred::DeferredEntry {
                    id: id.clone(),
                    component,
                    reason,
                    review,
                    recorded: today(),
                },
            );
            store.save_deferred(&r)?;
            Ok(format!(
                "deferred `{id}` registered — status surfaces it; overdue reviews cost a debt"
            ))
        })
    }

    /// List the declared deferrals.
    async fn deferred_list(&self) -> Result<String, String> {
        self.call(|store| {
            let r = store.load_deferred()?;
            if r.entries.is_empty() {
                return Ok("debt register empty — no deliberate deferrals declared".to_string());
            }
            let today = today();
            let roadmap = store.load_roadmap().unwrap_or_default();
            let version = harna_core::enforcement::validate::artifacts::deprecation::project_version(
                &self.root,
            )
            .unwrap_or_default();
            Ok(r.entries
                .iter()
                .map(|e| {
                    use harna_core::schema::registers::ledgers::deferred as dfr;
                    let declared = |id: &str| roadmap.find(id).is_some();
                    let kind = dfr::kind_in(&e.review, &declared);
                    let due = dfr::is_due(
                        &e.review,
                        &version,
                        &today,
                        &|id| roadmap.reached(id),
                        &declared,
                    );
                    format!(
                        "- [{}] `{}` review {}{}{}\n  why: {}",
                        e.id,
                        e.component,
                        e.review,
                        if due {
                            " — OVERDUE (costs a debt)"
                        } else if kind
                            == harna_core::schema::registers::ledgers::deferred::ReviewKind::Milestone
                        {
                            " (milestone)"
                        } else {
                            ""
                        },
                        if e.recorded.is_empty() {
                            String::new()
                        } else {
                            format!(" (recorded {})", e.recorded)
                        },
                        e.reason
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        })
    }

    /// `harna adopt`'s MCP name. `harna_adapt` (the original, misspelled
    /// name) stays as an alias so existing agent configs keep working.
    #[tool(
        description = "Draft a manifest from the tree (by_dir, exclude); re-run refreshes an adopted repo."
    )]
    async fn harna_adopt(
        &self,
        Parameters(AdoptRequest { by_dir, exclude }): Parameters<AdoptRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let (summary, body) = harna_core::generation::structure::adopt::run(
                &self.root,
                &store,
                harna_core::search::digest::MAX_FILES,
                by_dir.unwrap_or(false),
                &exclude.unwrap_or_default(),
            )?;
            Ok(format!("{summary}\n\n{body}"))
        })
    }

    /// Draft a manifest proposal from the existing tree.
    #[tool(description = "Alias of harna_adopt (kept for existing configs).")]
    async fn harna_adapt(&self) -> Result<String, String> {
        self.call(|store| {
            let (summary, body) = harna_core::generation::structure::adopt::run(
                &self.root,
                &store,
                harna_core::search::digest::MAX_FILES,
                false,
                &[],
            )?;
            Ok(format!("{summary}\n\n{body}"))
        })
    }

    /// Emit the generated human views from .harna/ data.
    #[tool(
        description = "Regenerate ARCHITECTURE.md and SURFACE.md. check = CI staleness gate; surface = SURFACE.md only."
    )]
    async fn harna_render(
        &self,
        Parameters(RenderRequest { check, surface }): Parameters<RenderRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let manifest = store.load_manifest()?;
            if manifest.is_empty() {
                anyhow::bail!("manifest has no layers — nothing to render yet");
            }
            if surface.unwrap_or(false) {
                let digest = store.load_digest()?;
                let stale = harna_core::generation::views::render::surface_is_stale(
                    &self.root, &manifest, &digest,
                )?;
                if stale {
                    if check.unwrap_or(false) {
                        anyhow::bail!("SURFACE.md is stale — render and commit the view");
                    }
                    let path = harna_core::generation::views::render::write_surface(
                        &self.root, &manifest, &digest,
                    )?;
                    return Ok(format!("rendered {}", path.display()));
                }
                return Ok("SURFACE.md already current (nothing to write)".into());
            }
            // CODEOWNERS and scoped files first: they are current or stale
            // on their own, whatever ARCHITECTURE.md says (0.7.7)
            let write = !check.unwrap_or(false);
            let mut out = Vec::new();
            {
                use harna_core::generation::views::codeowners::{sync, Owners};
                match sync(&self.root, &manifest, write)? {
                    Some(Owners::Stale) => {
                        anyhow::bail!("{}", Owners::Stale.note().unwrap_or_default())
                    }
                    Some(o) => out.extend(o.note()),
                    None => {}
                }
            }
            for (f, stale) in harna_core::generation::views::agents::scoped::sync(
                &self.root,
                &manifest,
                &store.load_effective_rules()?,
                write,
            )? {
                if stale && !write {
                    anyhow::bail!("scoped instructions are stale in {f} — render and commit");
                }
                if stale {
                    out.push(format!("rendered {f}"));
                }
            }
            let digest_ok = store.load_digest().ok();
            let arch_stale =
                harna_core::generation::views::render::is_stale(&self.root, &manifest)?;
            let surface_state = digest_ok.as_ref().map(|d| {
                harna_core::generation::views::render::surface_is_stale(&self.root, &manifest, d)
                    .unwrap_or(true)
            });
            let stale = arch_stale || surface_state.unwrap_or(false);
            if stale && check.unwrap_or(false) {
                anyhow::bail!("generated views are stale — render and commit the views");
            }
            if !stale {
                out.push("views are current (nothing to write)".into());
                return Ok(out.join("\n"));
            }
            let path = harna_core::generation::views::render::write(&self.root, &manifest)?;
            out.push(format!("rendered {}", path.display()));
            if let (Some(digest), Some(true)) = (digest_ok.as_ref(), surface_state) {
                let path = harna_core::generation::views::render::write_surface(
                    &self.root, &manifest, digest,
                )?;
                out.push(format!("rendered {}", path.display()));
            }
            Ok(out.join("\n"))
        })
    }

    fn call<F>(&self, f: F) -> Result<String, String>
    where
        F: FnOnce(Store) -> anyhow::Result<String>,
    {
        // Failures are tool results with isError: true (MCP spec), not
        // JSON-RPC protocol errors — the model sees the message.
        let store = self.store().map_err(|e| format!("{e:#}"))?;
        f(store).map_err(|e| format!("{e:#}"))
    }
}

/// ISO date (`YYYY-MM-DD`, local) — the same stamp every CLI record
/// carries, so a rule's origin reads the same whichever surface wrote it.
/// `HARNA_DATE` pins it for reproducible runs.
pub(crate) fn today() -> String {
    std::env::var("HARNA_DATE")
        .unwrap_or_else(|_| chrono::Local::now().format("%Y-%m-%d").to_string())
}
