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
        other => anyhow::bail!("unknown hook event `{other}` — session-start | post-edit | stop"),
    }
}

/// The onboarding digest, so a fresh session starts briefed whether or
/// not the agent remembers to ask.
pub fn session_start(store: &Store) -> anyhow::Result<Reply> {
    if !store.exists() {
        return Ok(Reply::default());
    }
    let onb = crate::composition::briefing::onboarding::build(store)?;
    Ok(Reply { code: 0, stdout: onb.text, stderr: String::new() })
}

/// The blocking findings for the current changes, if any.
fn blocking(root: &Path, store: &Store) -> Option<Vec<String>> {
    if !store.exists() {
        return None;
    }
    // Not a git repo (or git missing): --changed has no meaning; stay quiet.
    let files = validate::changed_rust_files(root).ok()?;
    let found = validate::run_on(root, store, 10_000, Some(files), None).ok()?;
    let ledger = store.load_acknowledged().unwrap_or_default();
    let report = acknowledge::gate(&ledger, found, false);
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
        out.push_str(&format!("… and {} more (harna validate --changed)\n", errors.len() - SHOWN));
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

/// Before the agent stops: a blocked change sends it back — once. When the
/// runtime says this stop is already a retry (`stop_hook_active`), harna
/// lets it go and says so, rather than looping the agent forever.
pub fn stop(root: &Path, store: &Store, input: &str) -> Reply {
    let retry = serde_json::from_str::<serde_json::Value>(input)
        .ok()
        .and_then(|v| v.get("stop_hook_active").and_then(|b| b.as_bool()))
        .unwrap_or(false);
    let Some(errors) = blocking(root, store) else { return Reply::default() };
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
        let root = std::env::temp_dir().join(format!("harna-events-{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(&root).unwrap();
        for ev in ["session-start", "post-edit", "stop"] {
            assert_eq!(answer(&root, &store, ev, "").unwrap(), Reply::default(), "{ev}");
        }
        assert!(answer(&root, &store, "pre-lunch", "").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
