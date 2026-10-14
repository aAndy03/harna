//! harna-mcp: an MCP stdio server exposing harna's memory to any agent.
//!
//! Tools:
//! - harna_onboarding   — the ~1k-token digest a fresh session starts with
//! - harna_rules        — rules for (or all) scope(s), token-efficient
//! - harna_add_rule     — persist a new rule (survives compaction)
//! - harna_log_decision — persist a decision to .harna/decisions/
//! - harna_scan         — rebuild the compressed code map

mod tools;

use anyhow::Context as _;
use rmcp::service::serve_server;
use tokio::io::stdin;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = tools::HarnaTools::new().context("creating harna tools service")?;
    let server = serve_server(service, (stdin(), tokio::io::stdout()))
        .await
        .context("serving MCP over stdio")?;
    server.waiting().await?;
    Ok(())
}
