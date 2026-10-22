//! Doctor section (split 1.1.8): what harna leans on outside `.harna/` —
//! the map, git, the commit gate, the agent instructions and wiring.

use std::path::Path;

use super::{ok, warn, CheckReport};
use crate::schema::store::Store;

/// The map, git, the hooks and the agent wiring.
pub(super) fn environment(
    store: &Store,
    root: &Path,
    checks: &mut Vec<CheckReport>,
) -> anyhow::Result<()> {
    // Digest freshness: present, parseable, not outrun by changed files.
    match store.load_digest() {
        // A map written by a newer harna than the one running (0.3.47):
        // hooks call whatever `harna` is on PATH, and an older binary
        // misreads a newer map (this repo's 0.3.41 hook reported cycles
        // the 0.3.46 extractor had already dissolved).
        Ok(d) if d.extractor_version > crate::search::digest::EXTRACTOR_VERSION => {
            checks.push(warn(
                "binary-version",
                &format!(
                    "this harna ({}) is older than the one that wrote the map (extractor {} > {}) — its readings are stale",
                    env!("CARGO_PKG_VERSION"),
                    d.extractor_version,
                    crate::search::digest::EXTRACTOR_VERSION
                ),
                "update the installed binary (`cargo install harna-cli`, or `--path crates/harna-cli` in a harna checkout) — hooks run the one on PATH",
            ));
        }
        Ok(d) => {
            let stamp = chrono::DateTime::parse_from_rfc3339(&d.generated_at).ok();
            let stale_files = stamp
                .map(|st| {
                    crate::search::digest::respected_rust_files(
                        root,
                        crate::search::digest::MAX_FILES,
                    )
                    .into_iter()
                    .filter(|rel| {
                        std::fs::metadata(root.join(rel))
                            .and_then(|m| m.modified())
                            .map(|mt| {
                                let utc: chrono::DateTime<chrono::Utc> = mt.into();
                                utc > st.with_timezone(&chrono::Utc)
                            })
                            .unwrap_or(false)
                    })
                    .count()
                })
                .unwrap_or(0);
            if stale_files == 0 {
                checks.push(ok(
                    "digest",
                    &format!("{} modules, current", d.module_count()),
                ));
            } else {
                checks.push(warn(
                    "digest",
                    &format!("{} module(s) changed since the last scan", stale_files),
                    "run `harna scan` (incremental — only the changed files are re-parsed)",
                ));
            }
        }
        Err(_) => checks.push(warn("digest", "missing or unparseable", "run `harna scan`")),
    }

    // Git: parity, dependents, diff, and --changed all lean on it.
    let git_ok = std::process::Command::new("git")
        .stdin(std::process::Stdio::null())
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if git_ok {
        checks.push(ok("git", "repository present"));
    } else {
        checks.push(warn(
            "git",
            "no repository — parity events, dependents diff and --changed need it",
            "run `git init` and commit .harna/ with the code it describes",
        ));
    }

    // Commit gate: does the pre-commit hook invoke harna?
    if git_ok {
        let hooks_path = std::process::Command::new("git")
            .stdin(std::process::Stdio::null())
            .args(["config", "core.hooksPath"])
            .current_dir(root)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        let hook_dir = hooks_path.unwrap_or_default();
        let hook_file = if hook_dir.is_empty() {
            root.join(".git/hooks/pre-commit")
        } else {
            root.join(&hook_dir).join("pre-commit")
        };
        match std::fs::read_to_string(&hook_file) {
            Ok(body) if body.contains("harna") || body.contains("harna.exe") => {
                checks.push(ok("commit-gate", "pre-commit hook wired to harna"));
            }
            _ => checks.push(warn(
                "commit-gate",
                "no pre-commit hook invoking harna",
                "run `harna hooks install` (git pre-commit + core.hooksPath, Claude Code hooks, CI)",
            )),
        }
    }

    // Agent instructions: does AGENTS.md exist and speak about harna?
    let agents = std::fs::read_to_string(root.join("AGENTS.md"));
    match agents {
        Ok(body) if body.to_lowercase().contains("harna") => {
            checks.push(ok(
                "agent-instructions",
                "AGENTS.md carries the harna contract",
            ));
        }
        Ok(_) => checks.push(warn(
            "agent-instructions",
            "AGENTS.md exists but does not mention harna",
            "run `harna init --agents` to append the managed harna block (your prose stays)",
        )),
        Err(_) => checks.push(warn(
            "agent-instructions",
            "no AGENTS.md — agents start every session blind",
            "run `harna init --agents` (writes the stock instruction file)",
        )),
    }

    // Agent wiring: is harna referenced by a local MCP config, or is the
    // MCP server binary installed? 0.3.32 dogfood: the warn was
    // unsatisfiable on repos whose agents use the CLI — the server ships
    // (harna-mcp), it just was never looked for.
    let wired = ["opencode.json", ".mcp.json"]
        .iter()
        .map(|f| root.join(f))
        .any(|p| {
            std::fs::read_to_string(p)
                .map(|body| body.contains("harna"))
                .unwrap_or(false)
        });
    let server = std::process::Command::new("harna-mcp")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let server_installed = server.is_some();
    // the server agents talk to is this harna's (0.9.2, field test: a
    // 0.3.37 harna-mcp on PATH beside harna 0.9.1 went unnoticed)
    if let Some(v) = &server {
        let mine = env!("CARGO_PKG_VERSION");
        if !v.ends_with(mine) {
            checks.push(warn(
                "mcp-version",
                &format!(
                    "harna-mcp on PATH reports `{v}`, this harna is {mine} — agents get an older toolbox"
                ),
                "cargo install --path crates/harna-mcp (or the release) beside harna",
            ));
        }
    }
    // the map's line endings (0.9.5): a map git rewrites on checkout reads
    // as changed after every no-op scan
    if store.digest_dir().is_dir() && !store.map_eol_pinned() {
        checks.push(warn(
            "map-eol",
            &format!("`.gitattributes` does not pin the code map (`{}`): under core.autocrlf, git shows the map changed after a no-op scan", crate::schema::store::MAP_EOL_LINE),
            "harna init (adds the line; nothing else changes)",
        ));
    }
    if wired {
        checks.push(ok("agent-wiring", "local MCP config mentions harna"));
    } else if server_installed {
        checks.push(ok(
            "agent-wiring",
            "harna-mcp server binary on PATH (one config line wires it)",
        ));
    } else {
        checks.push(warn(
            "agent-wiring",
            "no local MCP config mentions harna and no harna-mcp binary on PATH",
            "install harna-mcp alongside harna, then add the MCP server to opencode.json / .mcp.json so agents load the tools",
        ));
    }

    Ok(())
}
