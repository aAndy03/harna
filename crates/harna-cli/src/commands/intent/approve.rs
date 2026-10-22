//! `harna approve` and `harna approvals` (1.1.4): the owner's yes, as data.
//!
//! CLI only on purpose: the MCP tools never approve, so an agent asks and
//! the owner answers. harna cannot prove who typed the command; the docs
//! say so.

use std::path::Path;

use clap::Subcommand;

use harna_core::enforcement::validate::approvals as check;
use harna_core::schema::registers::approvals::{Approval, Require};
use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum ApprovalsCmd {
    /// Every approval with its state (current, changed, gone), then the
    /// changes that need the owner's yes.
    List,
    /// Name a change that needs an approval before it lands.
    Require {
        /// dependency-added | path
        kind: String,
        /// For `path`: the globs it covers. For `dependency-added`: the
        /// manifests it reads (all when omitted).
        #[arg(long, num_args = 1..)]
        paths: Vec<String>,
        /// Why the owner wants to be asked.
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Drop a requirement.
    Unrequire {
        kind: String,
        #[arg(long, num_args = 1..)]
        paths: Vec<String>,
    },
    /// Withdraw an approval.
    Revoke { id: String },
}

/// What `harna approve` records.
pub struct Approve {
    pub id: String,
    pub paths: Vec<String>,
    pub note: String,
    pub by: Option<String>,
    pub screenshot: String,
}

fn git_user(root: &Path) -> String {
    std::process::Command::new("git")
        .stdin(std::process::Stdio::null())
        .args(["config", "user.name"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// A second barrier behind the Claude hook (review of 1.1.4): an agent
/// host marks the shells it runs (`CLAUDECODE=1`; any host can set
/// `HARNA_AGENT=1`), and the owner's yes is refused there.
/// `command` is what the owner runs instead, quoted back to the agent so
/// it can hand it over (1.1.9 field test: the refusal named no command).
fn refuse_in_agent_shell(command: &str) -> anyhow::Result<()> {
    let set = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty() && v != "0");
    if set("CLAUDECODE") || set("HARNA_AGENT") {
        anyhow::bail!(
            "this shell belongs to an agent: this is the owner's to do. Ask the owner to run, in their own terminal:\n  {command}"
        );
    }
    Ok(())
}

pub fn approve(store: &Store, root: &Path, a: Approve) -> anyhow::Result<()> {
    refuse_in_agent_shell(&format!(
        "harna approve {}{}",
        a.id,
        if a.paths.is_empty() {
            String::new()
        } else {
            format!(" --paths {}", a.paths.join(" "))
        }
    ))?;
    let mut reg = store.load_approvals()?;
    let dependency = a.id.starts_with("dep:");
    if dependency && !a.paths.is_empty() {
        anyhow::bail!("a dependency approval names a package, not files: drop --paths");
    }
    // a re-approval keeps the paths it had unless new ones are given
    let paths = if a.paths.is_empty() && !dependency {
        reg.find(&a.id)
            .map(|p| p.paths.clone())
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "say what `{}` freezes: harna approve {} --paths <file or glob>..",
                    a.id,
                    a.id
                )
            })?
    } else {
        a.paths.iter().map(|p| p.replace('\\', "/")).collect()
    };
    let files = check::files_of(root, &paths);
    if !dependency && files.is_empty() {
        anyhow::bail!(
            "`{}` matches no file — nothing to approve",
            paths.join(", ")
        );
    }
    let stored = reg.approve(Approval {
        id: a.id.clone(),
        hash: if dependency {
            String::new()
        } else {
            check::hash(root, &files)
        },
        paths,
        version: 1,
        by: a.by.unwrap_or_else(|| git_user(root)),
        date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        note: a.note,
        screenshot: a.screenshot,
    });
    let label = stored.label();
    store.save_approvals(&reg)?;
    if dependency {
        println!("approved `{label}`: the dependency may land");
    } else {
        println!(
            "approved `{label}` — {} file(s) frozen; a later change to them is a debt until the owner approves again",
            files.len()
        );
    }
    Ok(())
}

pub fn run(store: &Store, root: &Path, cmd: Option<ApprovalsCmd>) -> anyhow::Result<()> {
    let mut reg = store.load_approvals()?;
    let cmd = cmd.unwrap_or(ApprovalsCmd::List);
    // adding a requirement only restricts, so an agent may set one up;
    // loosening (unrequire, revoke) is the owner's (1.1.9 field test)
    match &cmd {
        ApprovalsCmd::Unrequire { kind, paths } => refuse_in_agent_shell(&format!(
            "harna approvals unrequire {kind}{}",
            if paths.is_empty() {
                String::new()
            } else {
                format!(" --paths {}", paths.join(" "))
            }
        ))?,
        ApprovalsCmd::Revoke { id } => {
            refuse_in_agent_shell(&format!("harna approvals revoke {id}"))?
        }
        _ => {}
    }
    match cmd {
        ApprovalsCmd::List => {
            if reg.is_empty() {
                println!("no approvals and no requirements — `harna approve <id> --paths ..` freezes what the owner approved");
                return Ok(());
            }
            for a in &reg.approvals {
                let state = match check::state(root, a).0 {
                    check::State::Dependency => "dependency",
                    check::State::Current => "current",
                    check::State::Changed => "CHANGED",
                    check::State::Gone => "GONE",
                };
                let who = [a.by.as_str(), a.date.as_str()]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "{:24} {state:10} {who}{}",
                    a.label(),
                    if a.paths.is_empty() {
                        String::new()
                    } else {
                        format!(" — {}", a.paths.join(", "))
                    }
                );
            }
            if !reg.require.is_empty() {
                println!("needs the owner's yes first:");
                for r in &reg.require {
                    println!("  - {}", r.line());
                }
            }
        }
        ApprovalsCmd::Require {
            kind,
            paths,
            reason,
        } => {
            let r = Require {
                kind,
                paths,
                reason,
            };
            let line = r.line();
            reg.require(r)?;
            store.save_approvals(&reg)?;
            println!("required: {line} — validate names it when it lands unapproved");
        }
        ApprovalsCmd::Unrequire { kind, paths } => {
            let before = reg.require.len();
            reg.require
                .retain(|r| !(r.kind == kind && r.paths == paths));
            if reg.require.len() == before {
                anyhow::bail!("no `{kind}` requirement with those paths (harna approvals)");
            }
            store.save_approvals(&reg)?;
            println!("requirement dropped");
        }
        ApprovalsCmd::Revoke { id } => {
            reg.revoke(&id)?;
            store.save_approvals(&reg)?;
            println!("approval `{id}` withdrawn");
        }
    }
    Ok(())
}
