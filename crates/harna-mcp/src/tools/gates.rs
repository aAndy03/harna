//! MCP tools for the gates and planners the CLI already had: status,
//! preflight, validate-intent, the advisory ledger, decompose — the
//! AGENTS.md ritual's steps, callable without a shell.

use std::future::Future;

use rmcp::{handler::server::tool::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{required, today, HarnaTools};

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PreflightRequest {
    /// Max files to check (a safety cap).
    pub max_files: Option<u32>,
    /// End of a session instead: refresh map and views, check the change, list what is left.
    pub finish: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct BaselineRequest {
    /// record | tighten | show
    pub action: String,
    /// record: only this check.
    pub check: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct AcknowledgeRequest {
    /// "add" | "remove" | "list".
    pub action: String,
    /// The check that fired, e.g. "no-copy-paste" (add/remove).
    pub check: Option<String>,
    /// The repo-relative file it fired on (add/remove).
    pub file: Option<String>,
    /// Why the advisory is accepted rather than fixed (add).
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct HooksRequest {
    /// "status" | "install".
    pub action: String,
    /// Targets to install: git | claude | ci (default all).
    pub targets: Option<Vec<String>>,
    /// Command the Claude Code hooks run (default "harna").
    pub bin: Option<String>,
    /// Replace files not written by harna.
    pub force: Option<bool>,
    /// Add Rust craft gates (fmt, clippy, tests) to git and CI.
    pub craft: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DecomposeRequest {
    /// Repo-relative file to plan, e.g. "crates/x/src/main.rs".
    pub file: String,
}

/// A repo-relative path that stays inside the repo (tools read files by it).
pub(super) fn repo_relative(file: &str) -> anyhow::Result<&str> {
    use std::path::Component as C;
    let inside = !file.is_empty()
        && !file.contains(':')
        && !file.starts_with(['/', '\\'])
        && std::path::Path::new(file)
            .components()
            .all(|c| matches!(c, C::Normal(_) | C::CurDir));
    if !inside {
        anyhow::bail!("`{file}` must be a repo-relative path inside the repository");
    }
    Ok(file)
}

#[tool_router(router = gates_router, vis = "pub(super)")]
impl HarnaTools {
    #[tool(
        description = "One-line health: map, manifest, tests, score, rules, parity, deferrals, roadmap NOW, handoff."
    )]
    async fn harna_status(&self) -> Result<String, String> {
        self.call(|store| Ok(harna_core::composition::briefing::status::build(&store)?.render()))
    }

    #[tool(
        description = "Release gate: validation + doctor + parity + map → PASS/FAIL (no test run). finish=true: end-of-session checklist."
    )]
    async fn harna_preflight(
        &self,
        Parameters(PreflightRequest { max_files, finish }): Parameters<PreflightRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            if finish.unwrap_or(false) {
                return Ok(
                    harna_core::composition::gates::finish::run(&store, &self.root)?.render(),
                );
            }
            let r = harna_core::composition::gates::preflight::build(
                &store,
                &self.root,
                max_files.map_or(harna_core::search::digest::MAX_FILES, |m| m as usize),
            )?;
            Ok(r.render())
        })
    }

    #[tool(
        description = "Claims vs state: package version, changelog head, roadmap, parity silence. Each DRIFT names both sides."
    )]
    async fn harna_validate_intent(&self) -> Result<String, String> {
        self.call(move |store| {
            Ok(harna_core::composition::gates::intent::build(&store, &self.root)?.render())
        })
    }

    #[tool(
        description = "Ratchet ledger: 'record' freezes today's errors per check+file, 'tighten' lowers them, 'show'."
    )]
    async fn harna_baseline(
        &self,
        Parameters(BaselineRequest { action, check }): Parameters<BaselineRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            harna_core::enforcement::validate::ratchet::command(
                &self.root,
                &store,
                &action,
                check.as_deref(),
            )
        })
    }

    #[tool(
        description = "Advisory ledger: add (check, file, reason) accepts an Info finding; remove; list. Not for errors."
    )]
    async fn harna_acknowledge(
        &self,
        Parameters(AcknowledgeRequest {
            action,
            check,
            file,
            reason,
        }): Parameters<AcknowledgeRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut a = store.load_acknowledged()?;
            match action.as_str() {
                "add" => {
                    let check = required(check, "check", "add")?;
                    a.upsert(harna_core::schema::registers::ledgers::acknowledged::Acknowledgement {
                        check: check.clone(),
                        file: required(file, "file", "add")?,
                        reason: required(reason, "reason", "add")?,
                        recorded: today(),
                    });
                    store.save_acknowledged(&a)?;
                    Ok(format!(
                        "advisory `{check}` locked for its file ({}) — Infos only; an error by that name still gates",
                        a.entries.len()
                    ))
                }
                "remove" => {
                    let check = required(check, "check", "remove")?;
                    let file = required(file, "file", "remove")?;
                    if !a.remove(&check, &file) {
                        anyhow::bail!("no acknowledgment for `{check}` on `{file}`");
                    }
                    store.save_acknowledged(&a)?;
                    Ok("acknowledgment removed — the advisory nags again".into())
                }
                "list" if a.entries.is_empty() => Ok("advisory ledger empty".into()),
                "list" => Ok(a
                    .entries
                    .iter()
                    .map(|e| format!("- [{}] {} — {}", e.check, e.file, e.reason))
                    .collect::<Vec<_>>()
                    .join("\n")),
                other => anyhow::bail!("unknown action `{other}` — add | remove | list"),
            }
        })
    }

    #[tool(
        description = "Hooks: 'status'; 'install' writes git pre-commit, Claude Code hooks, CI (no overwrite without force)."
    )]
    async fn harna_hooks(
        &self,
        Parameters(HooksRequest {
            action,
            targets,
            bin,
            force,
            craft,
        }): Parameters<HooksRequest>,
    ) -> Result<String, String> {
        use harna_core::generation::hooks::Target;
        let bin = bin.unwrap_or_else(|| "harna".into());
        self.call(move |_store| match action.as_str() {
            "status" => Ok(harna_core::composition::hooks::setup::status(
                &self.root, &bin,
            )),
            "install" => {
                let targets = match targets {
                    None => Target::ALL.to_vec(),
                    Some(ts) => ts
                        .iter()
                        .map(|t| Target::parse(t))
                        .collect::<anyhow::Result<_>>()?,
                };
                let r = harna_core::composition::hooks::setup::install(
                    &self.root,
                    &targets,
                    &bin,
                    force.unwrap_or(false),
                    craft.unwrap_or(false),
                )?;
                Ok(r.render())
            }
            other => anyhow::bail!("unknown action `{other}` — status | install"),
        })
    }

    #[tool(description = "How to split an oversized file (read-only plan + manifest sketch).")]
    async fn harna_decompose(
        &self,
        Parameters(DecomposeRequest { file }): Parameters<DecomposeRequest>,
    ) -> Result<String, String> {
        self.call(move |_store| {
            let file = repo_relative(&file)?;
            let src = std::fs::read_to_string(self.root.join(file)).map_err(|_| {
                anyhow::anyhow!("cannot read `{file}` — repo-relative path from the repo root")
            })?;
            Ok(harna_core::composition::decompose::plan(file, &src).render())
        })
    }
}
