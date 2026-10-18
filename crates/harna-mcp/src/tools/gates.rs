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
    /// Max files to check; defaults to 10_000.
    pub max_files: Option<u32>,
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
    #[tool(description = "One-shot health line: map size, manifest, test coverage, score, rules, open parity debts, deferred register, roadmap NOW, and any recorded handoff. Cheap — call it to orient.")]
    async fn harna_status(&self) -> Result<String, String> {
        self.call(|store| Ok(harna_core::composition::status::build(&store)?.render()))
    }

    #[tool(description = "Release gate as one call: validation errors + doctor failures + open parity debts + map freshness compose into PASS/FAIL (the report's last line). The test suite is NOT run (harna never executes project code) — run it yourself before releasing.")]
    async fn harna_preflight(
        &self,
        Parameters(PreflightRequest { max_files }): Parameters<PreflightRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let r = harna_core::composition::preflight::build(
                &store,
                &self.root,
                max_files.unwrap_or(10_000) as usize,
            )?;
            Ok(r.render())
        })
    }

    #[tool(description = "Truthfulness reconciliation: package version, changelog head, roadmap NOW/DONEs and parity silence cross-checked against the state. Each DRIFT names both sides; reconcile before claiming coherence.")]
    async fn harna_validate_intent(&self) -> Result<String, String> {
        self.call(move |store| Ok(harna_core::composition::intent::build(&store, &self.root)?.render()))
    }

    #[tool(description = "The advisory ledger: action='add' locks an accepted Info finding (check + file + reason) so it renders as a receipt and stays out of strict gating; 'remove' re-arms it; 'list' shows what is locked. Errors are never acknowledgeable.")]
    async fn harna_acknowledge(
        &self,
        Parameters(AcknowledgeRequest { action, check, file, reason }): Parameters<AcknowledgeRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut a = store.load_acknowledged()?;
            match action.as_str() {
                "add" => {
                    let check = required(check, "check", "add")?;
                    a.upsert(harna_core::schema::acknowledged_data::Acknowledgement {
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

    #[tool(description = "Split planner for an oversized file: maps its top-level items and returns HOW to split it (dispatch-split or prefix-cluster) plus the manifest sketch. Read-only — applying the split stays a deliberate edit.")]
    async fn harna_decompose(
        &self,
        Parameters(DecomposeRequest { file }): Parameters<DecomposeRequest>,
    ) -> Result<String, String> {
        self.call(move |_store| {
            let file = repo_relative(&file)?;
            let src = std::fs::read_to_string(self.root.join(file))
                .map_err(|_| anyhow::anyhow!("cannot read `{file}` — repo-relative path from the repo root"))?;
            Ok(harna_core::composition::decompose::plan(file, &src).render())
        })
    }
}
