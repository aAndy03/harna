//! Component `hook-events` (0.3.36): what `harna hook <event>` answers
//! when an agent runtime (Claude Code) calls it.
//!
//! Claude Code's contract: stdout of a SessionStart hook joins the
//! session's context; exit code 2 from PostToolUse feeds stderr back to
//! the agent; exit code 2 from Stop keeps the agent working with stderr
//! as the reason. Every answer here is quiet when there is nothing to say
//! — a hook that talks on every edit is noise the agent learns to skip.

use std::path::Path;

use crate::enforcement::validate::{self, acknowledge, Severity};
use crate::schema::store::Store;

/// One hook answer: the process exit code plus its two streams.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reply {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Errors shown per reply; the rest are counted.
const SHOWN: usize = 8;

/// Dispatch by event name; `input` is the hook's JSON stdin (may be empty).
pub fn answer(root: &Path, store: &Store, event: &str, input: &str) -> anyhow::Result<Reply> {
    match event {
        "session-start" => session_start(store),
        "post-edit" => Ok(post_edit(root, store)),
        "stop" => Ok(stop(root, store, input)),
        "pre-bash" => Ok(pre_bash(input)),
        "pre-edit" => Ok(pre_edit(input)),
        other => anyhow::bail!(
            "unknown hook event `{other}` — session-start | post-edit | stop | pre-bash | pre-edit"
        ),
    }
}

/// The onboarding digest, so a fresh session starts briefed whether or
/// not the agent remembers to ask.
pub fn session_start(store: &Store) -> anyhow::Result<Reply> {
    if !store.exists() {
        return Ok(Reply::default());
    }
    let onb = crate::composition::briefing::onboarding::build(store)?;
    Ok(Reply {
        code: 0,
        stdout: onb.text,
        stderr: String::new(),
    })
}

/// The blocking findings for the current changes, if any.
fn blocking(root: &Path, store: &Store) -> Option<Vec<String>> {
    if !store.exists() {
        return None;
    }
    // Not a git repo (or git missing): --changed has no meaning; stay quiet.
    let files = validate::changed_rust_files(root).ok()?;
    let found = validate::run_on(
        root,
        store,
        crate::search::digest::MAX_FILES,
        Some(files),
        None,
    )
    .ok()?;
    let report = acknowledge::gate_for(store, found, false);
    if !report.blocks {
        return None;
    }
    Some(
        report
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .map(|v| v.render())
            .collect(),
    )
}

fn listed(errors: &[String]) -> String {
    let mut out = String::new();
    for e in errors.iter().take(SHOWN) {
        out.push_str(e);
        out.push('\n');
    }
    if errors.len() > SHOWN {
        out.push_str(&format!(
            "… and {} more (harna validate --changed)\n",
            errors.len() - SHOWN
        ));
    }
    out
}

/// After an edit: silent when the change conforms; otherwise the errors,
/// fed straight back to the agent while the edit is fresh.
pub fn post_edit(root: &Path, store: &Store) -> Reply {
    match blocking(root, store) {
        None => Reply::default(),
        Some(errors) => Reply {
            code: 2,
            stdout: String::new(),
            stderr: format!(
                "harna: this change breaks {} conformance rule(s) — fix before moving on:\n{}",
                errors.len(),
                listed(&errors)
            ),
        },
    }
}

/// Before the agent runs a shell command (1.1.4): approving is the owner's.
/// `harna approve` and the approval register's edits (require, unrequire,
/// revoke) are refused with the way forward; everything else passes
/// silently. Claude Code's PreToolUse contract: exit 2 blocks the call and
/// shows stderr to the agent. Quoted text is not a command (`rg "harna
/// approve"`), and each `;`/`&&`/`|` segment is read on its own.
pub fn pre_bash(input: &str) -> Reply {
    let command = serde_json::from_str::<serde_json::Value>(input)
        .ok()
        .and_then(|v| {
            v.get("tool_input")
                .and_then(|t| t.get("command"))
                .and_then(|c| c.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    if !runs_an_approval(&command) {
        return Reply::default();
    }
    refuse("approving is the owner's, not the agent's. Show the owner what changed and ask them to run the command in their own terminal; `harna approvals` lists what is approved and what needs a yes.")
}

/// Before the agent edits a file (1.1.4, review): the approvals register is
/// the owner's; an edit to it is refused like the command.
pub fn pre_edit(input: &str) -> Reply {
    let path = serde_json::from_str::<serde_json::Value>(input)
        .ok()
        .and_then(|v| {
            let t = v.get("tool_input")?;
            t.get("file_path")
                .or_else(|| t.get("notebook_path"))
                .and_then(|p| p.as_str())
                .map(|p| p.replace('\\', "/"))
        })
        .unwrap_or_default();
    if !path.ends_with(".harna/approvals.toml") {
        return Reply::default();
    }
    refuse("`.harna/approvals.toml` is the owner's register: ask the owner, who runs `harna approve` in their own terminal.")
}

fn refuse(why: &str) -> Reply {
    Reply {
        code: 2,
        stdout: String::new(),
        stderr: format!("harna: {why}\n"),
    }
}

/// Does a shell command run `harna approve` or edit the register?
pub fn runs_an_approval(command: &str) -> bool {
    let Ok(re) = regex::Regex::new(
        r"(?i)\bharna[\w.-]*\b.*?\s(?:approve\b|approvals\s+(?:\S+\s+)*?(?:unrequire|revoke)\b)",
    ) else {
        return false;
    };
    // drop quoted text: an argument that mentions the command is not it
    let mut plain = String::with_capacity(command.len());
    let mut quote: Option<char> = None;
    for ch in command.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None => plain.push(ch),
        }
    }
    plain
        .split([';', '|', '&', '\n'])
        .any(|segment| re.is_match(segment))
}

// Before the agent stops: a blocked change sends it back — once. When the
/// runtime says this stop is already a retry (`stop_hook_active`), harna
/// lets it go and says so, rather than looping the agent forever.
pub fn stop(root: &Path, store: &Store, input: &str) -> Reply {
    let retry = serde_json::from_str::<serde_json::Value>(input)
        .ok()
        .and_then(|v| v.get("stop_hook_active").and_then(|b| b.as_bool()))
        .unwrap_or(false);
    let Some(errors) = blocking(root, store) else {
        return Reply::default();
    };
    if retry {
        return Reply {
            code: 0,
            stdout: String::new(),
            stderr: format!(
                "harna: still {} blocking finding(s) — stopping anyway; tell the user the change is not committable yet\n",
                errors.len()
            ),
        };
    }
    Reply {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "harna: not done yet — the change is BLOCKED ({} error(s)). Fix them, or tell the user why you can't:\n{}",
            errors.len(),
            listed(&errors)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninitialized_repos_get_silence_and_unknown_events_an_error() {
        let root =
            std::env::temp_dir().join(format!("harna-events-{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(&root).unwrap();
        for ev in ["session-start", "post-edit", "stop"] {
            assert_eq!(
                answer(&root, &store, ev, "").unwrap(),
                Reply::default(),
                "{ev}"
            );
        }
        assert!(answer(&root, &store, "pre-lunch", "").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 1.1.4: the agent's shell never approves.
    #[test]
    fn approving_from_the_agents_shell_is_refused() {
        let call =
            |c: &str| pre_bash(&serde_json::json!({"tool_input": {"command": c}}).to_string());
        for c in [
            "harna approve calendar",
            "cd web && harna.exe approve dep:left-pad",
            "harna --root . approvals revoke calendar",
            "harna approvals unrequire path --paths 'db/**'",
        ] {
            assert_eq!(call(c).code, 2, "{c}");
        }
        // review of 1.1.4: the ways around the first regex
        for c in [
            "cargo run -p harna-cli -- approve calendar",
            "harna --root=. approve x",
            "HARNA.EXE approve x",
        ] {
            assert_eq!(call(c).code, 2, "{c}");
        }
        for c in [
            "harna approvals",
            "harna validate --changed",
            "echo approve",
            "rg \"harna approve\" docs",
            "git commit -m 'teach harna approve'",
            "",
        ] {
            assert_eq!(call(c), Reply::default(), "{c}");
        }
        assert_eq!(pre_bash("not json"), Reply::default());
        let edit =
            |p: &str| pre_edit(&serde_json::json!({"tool_input": {"file_path": p}}).to_string());
        assert_eq!(edit("C:\\repo\\.harna\\approvals.toml").code, 2);
        assert_eq!(edit("src/app.rs"), Reply::default());
    }
}
