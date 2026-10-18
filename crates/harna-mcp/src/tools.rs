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
        description = "Session start: architecture, active rules, glossary, roadmap NOW, handoff and code map in one digest. Call first, every session."
    )]
    async fn harna_onboarding(&self) -> Result<String, String> {
        self.call(|store| {
            let onb = onboarding::build(&store)?;
            Ok(onb.text)
        })
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
                max_files.unwrap_or(10_000) as usize,
                full.unwrap_or(false),
            )?;
            Ok(r.render())
        })
    }

    /// Check the codebase for conformance violations.
    #[tool(
        description = "Conformance check; ends with verdict PASS/BLOCKED. changed_only=true after edits; strict=true makes infos block; env scopes env-tagged rules."
    )]
    async fn harna_validate(
        &self,
        Parameters(ValidateRequest {
            max_files,
            changed_only,
            env,
            strict,
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
                max_files.unwrap_or(10_000) as usize,
                files,
                env.as_deref(),
            )?;
            // The same gate as the CLI: acknowledged advisories ride as
            // receipts, and the verdict line says whether anything blocks.
            let ledger = store.load_acknowledged().unwrap_or_default();
            let report = harna_core::enforcement::validate::acknowledge::gate(
                &ledger,
                violations,
                strict.unwrap_or(false),
            );
            Ok(report.render())
        })
    }

    /// Upgrade .harna/ data files to this tool's schema version.
    #[tool(
        description = "Upgrade .harna/ data to this harna's schema (verified backups first, logged as a decision). dry=true previews."
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
        description = "Move closed parity events, superseded decisions and old score snapshots (keep = newest N, default 12) into .harna/archive/. Nothing is deleted."
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

    /// Declare a parity fan-out template.
    #[tool(
        description = "Declare a parity template: a change type's expected fan-out ceiling (max_targets) over a scope, with rationale. Same id replaces."
    )]
    async fn harna_template_add(
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
    #[tool(description = "List parity templates: id, scope, ceiling, rationale.")]
    async fn harna_template_list(&self) -> Result<String, String> {
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
        description = "Draft (never written) collaboration notes: kind='review' (checklist for a parity event id or a diff from a ref) or kind='release' (decisions + parity since from, default HEAD)."
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

    /// Register a deliberate deferral.
    #[tool(
        description = "Register a deliberate deferral: component, reason, review (ISO date or version). Overdue reviews cost score. Same id replaces."
    )]
    async fn harna_deferred_add(
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
            r.upsert(harna_core::schema::registers::deferred::DeferredEntry {
                id: id.clone(),
                component,
                reason,
                review,
                recorded: today(),
            });
            store.save_deferred(&r)?;
            Ok(format!(
                "deferred `{id}` registered — status surfaces it; overdue reviews cost a debt"
            ))
        })
    }

    /// List the declared deferrals.
    #[tool(description = "List deferrals; past-review entries are flagged OVERDUE.")]
    async fn harna_deferred_list(&self) -> Result<String, String> {
        self.call(|store| {
            let r = store.load_deferred()?;
            if r.entries.is_empty() {
                return Ok("debt register empty — no deliberate deferrals declared".to_string());
            }
            let today = today();
            let roadmap = store.load_roadmap().unwrap_or_default();
            Ok(r.entries
                .iter()
                .map(|e| {
                    let kind = harna_core::schema::registers::deferred::review_kind(&e.review);
                    let due = match kind {
                        harna_core::schema::registers::deferred::ReviewKind::Milestone => {
                            harna_core::schema::registers::deferred::milestone_due(
                                &e.review,
                                &|id| roadmap.reached(id),
                            )
                        }
                        _ => harna_core::schema::registers::deferred::review_due(
                            &e.review,
                            env!("CARGO_PKG_VERSION"),
                            &today,
                        ),
                    };
                    format!(
                        "- [{}] `{}` review {}{}{}\n  why: {}",
                        e.id,
                        e.component,
                        e.review,
                        if due {
                            " — OVERDUE (costs a debt)"
                        } else if kind
                            == harna_core::schema::registers::deferred::ReviewKind::Milestone
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
        description = "Draft a manifest (TOML, printed, never written) from the folder tree of a repo with no manifest yet. Twin of `harna adopt`."
    )]
    async fn harna_adopt(&self) -> Result<String, String> {
        self.harna_adapt().await
    }

    /// Draft a manifest proposal from the existing tree.
    #[tool(description = "Alias of harna_adopt (kept for existing configs).")]
    async fn harna_adapt(&self) -> Result<String, String> {
        self.call(|store| {
            // Missing .harna/ = the fresh start adopt exists for.
            if self.root.join(".harna/manifest.toml").is_file() {
                let existing = store.load_manifest()?;
                if !existing.is_empty() {
                    anyhow::bail!(
                        "this repository already has a manifest ({} layers) — edit the manifest or use `harna scaffold` instead",
                        existing.layers.len()
                    );
                }
            }
            let d = harna_core::generation::structure::adopt::draft(&self.root, 10_000)?;
            let text = harna_core::generation::structure::adopt::render(&d)?;
            Ok(format!(
                "draft: {} module(s) across {} layer(s)/{} container(s); {} cross-crate edge(s) proposed as depends_on\n\n{}",
                d.module_count, d.layer_count, d.container_count, d.cross_crate_edges, text
            ))
        })
    }

    /// Emit the generated human views from .harna/ data.
    #[tool(
        description = "Regenerate ARCHITECTURE.md and SURFACE.md from manifest + map. check=true = CI staleness gate; surface=true = SURFACE.md only."
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
                return Ok("views are current (nothing to write)".into());
            }
            let mut out = Vec::new();
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
