//! Component `hooks` (0.3.36): the enforcement harna installs for itself.
//!
//! A gate an agent must remember to run is a suggestion. These files make
//! the gates run on their own:
//! - **git pre-commit** (`.githooks/pre-commit`): `harna validate
//!   --changed` before every commit. Fails *closed* — a missing harna
//!   binary blocks the commit with the fix, instead of waving it through
//!   (bypass once: `HARNA_SKIP_HOOK=1`).
//! - **Claude Code hooks** (`.claude/settings.json`, merged): onboarding
//!   injected at session start, violations reported right after an edit,
//!   a blocked change sent back once before the agent stops.
//! - **CI** (`.github/workflows/harna.yml`): validate + preflight on push
//!   and pull requests.
//!
//! Every file harna writes carries the `harna-managed` marker; a file
//! without it belongs to the user and is never overwritten (`force`
//! aside). Wiring git's `core.hooksPath` is a process call, so it lives
//! with the caller (composition::hook_setup); this module only writes.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub const MARK: &str = "harna-managed";
pub const PRE_COMMIT: &str = ".githooks/pre-commit";
pub const CLAUDE_SETTINGS: &str = ".claude/settings.json";
pub const CI_WORKFLOW: &str = ".github/workflows/harna.yml";

/// Which enforcement to install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Git,
    Claude,
    Ci,
}

impl Target {
    pub const ALL: [Target; 3] = [Target::Git, Target::Claude, Target::Ci];

    pub fn parse(word: &str) -> anyhow::Result<Target> {
        match word.trim() {
            "git" => Ok(Target::Git),
            "claude" => Ok(Target::Claude),
            "ci" => Ok(Target::Ci),
            other => anyhow::bail!("unknown hook target `{other}` — git | claude | ci"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Target::Git => "git",
            Target::Claude => "claude",
            Target::Ci => "ci",
        }
    }

    pub fn path(self) -> &'static str {
        match self {
            Target::Git => PRE_COMMIT,
            Target::Claude => CLAUDE_SETTINGS,
            Target::Ci => CI_WORKFLOW,
        }
    }
}

/// What installing one target did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Written,
    Updated,
    AlreadyCurrent,
    /// The file belongs to the user (no marker / not mergeable): untouched.
    Kept(String),
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub target: Target,
    pub path: PathBuf,
    pub action: Action,
}

impl Outcome {
    pub fn render(&self) -> String {
        let what = match &self.action {
            Action::Written => "installed".to_string(),
            Action::Updated => "updated".to_string(),
            Action::AlreadyCurrent => "already current".to_string(),
            Action::Kept(why) => format!("left untouched — {why}"),
        };
        format!("{:6} {} — {what}", self.target.name(), self.target.path())
    }
}

/// The pre-commit script. `HARNA_BIN` → repo-local build (harna's own
/// repo dogfoods its fresh binary) → `harna` on PATH. `craft` (0.3.40)
/// adds the Rust craft gates before conformance: `cargo fmt --check` and
/// `cargo clippy -D warnings` (bypass once: `HARNA_SKIP_CRAFT=1`).
pub fn pre_commit_script(craft: bool) -> String {
    let (tag, craft_block) = if craft {
        ("v1+craft", CRAFT_PRE_COMMIT)
    } else {
        ("v1", "")
    };
    format!(
        r#"#!/bin/sh
# {MARK}: pre-commit {tag} — written by `harna hooks install`; reinstalling overwrites it.
# The conformance gate: fails the commit when the changed files violate the
# repo's rules or leave parity debts open. Fails CLOSED when harna is missing.
[ -d ".harna" ] || exit 0
if [ "$HARNA_SKIP_HOOK" = "1" ]; then
  echo "harna: pre-commit gate skipped (HARNA_SKIP_HOOK=1)" >&2
  exit 0
fi
exe="$HARNA_BIN"
if [ -z "$exe" ]; then
  for c in target/release/harna.exe target/release/harna target/debug/harna.exe target/debug/harna; do
    if [ -x "$c" ]; then exe="$c"; break; fi
  done
fi
if [ -z "$exe" ] && command -v harna >/dev/null 2>&1; then exe=harna; fi
if [ -z "$exe" ]; then
  echo "harna: the pre-commit gate cannot run — no harna binary found." >&2
  echo "harna: install it (cargo install harna-cli) or point HARNA_BIN at it;" >&2
  echo "harna: bypass this once with HARNA_SKIP_HOOK=1 git commit ..." >&2
  exit 1
fi
{craft_block}exec "$exe" validate --changed
"#
    )
}

/// The craft gates, as shell: run only in a Cargo workspace.
const CRAFT_PRE_COMMIT: &str = r#"if [ -f Cargo.toml ] && [ "$HARNA_SKIP_CRAFT" != "1" ]; then
  if ! cargo fmt --all -- --check >/dev/null 2>&1; then
    echo "harna: craft gate — code is not formatted; run: cargo fmt --all" >&2
    exit 1
  fi
  if ! cargo clippy --workspace --all-targets --quiet -- -D warnings; then
    echo "harna: craft gate — clippy found warnings (fix them, or HARNA_SKIP_CRAFT=1 once)" >&2
    exit 1
  fi
fi
"#;

/// The craft job: format, lint as errors, and the test suite — the gates
/// `preflight` names as NOT RUN, run where they cannot be skipped.
const CRAFT_CI_JOB: &str = r#"  craft:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - name: Format
        run: cargo fmt --all -- --check
      - name: Lint (warnings are errors)
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Tests
        run: cargo test --workspace
"#;

pub fn ci_workflow(craft: bool) -> String {
    let (tag, craft_job) = if craft {
        ("v1+craft", CRAFT_CI_JOB)
    } else {
        ("v1", "")
    };
    format!(
        r#"# {MARK}: ci {tag} — written by `harna hooks install --target ci`.
# The local pre-commit gate already runs these checks on every commit, so
# CI runs only where that gate cannot reach: pull requests (code from
# clones without the hooks), release tags (one clean-machine check per
# release), and on demand — never again on every push.
name: harna
on:
  pull_request:
  push:
    tags: ["v*"]
  workflow_dispatch:
jobs:
  conformance:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Install harna (the version that wrote this workflow)
        run: |
          if [ -f crates/harna-cli/Cargo.toml ]; then
            cargo install --path crates/harna-cli --locked
          else
            cargo install harna-cli --locked --version {version}
          fi
      - name: Conformance (every file, every gate)
        run: harna validate
      - name: Release gate
        run: harna preflight
{craft_job}"#,
        // CI must judge with the same harna that wrote the rules: an
        // older binary does not know newer checks and reports them as
        // errors (0.3.40's first CI run installed 0.3.0 and failed so).
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// The Claude Code hook entries, keyed by event. `bin` is the command
/// the hooks run (`harna` on PATH by default).
fn claude_entries(bin: &str) -> Vec<(&'static str, Value)> {
    let cmd = |event: &str| json!({"type": "command", "command": format!("{bin} hook {event}")});
    vec![
        ("SessionStart", json!({"hooks": [cmd("session-start")]})),
        (
            "PostToolUse",
            json!({"matcher": "Edit|Write|MultiEdit|NotebookEdit", "hooks": [cmd("post-edit")]}),
        ),
        ("Stop", json!({"hooks": [cmd("stop")]})),
    ]
}

/// Merge harna's hooks into existing Claude Code settings, keeping every
/// key and hook the user already has. Returns the new text and whether
/// anything changed; errors when the existing file is not a JSON object
/// (it is then the user's to fix, never ours to rewrite).
pub fn merge_claude_settings(existing: Option<&str>, bin: &str) -> anyhow::Result<(String, bool)> {
    let mut root: Value = match existing.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => serde_json::from_str(raw)
            .map_err(|e| anyhow::anyhow!("{CLAUDE_SETTINGS} is not valid JSON ({e})"))?,
        None => json!({}),
    };
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{CLAUDE_SETTINGS} is not a JSON object"))?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("`hooks` in {CLAUDE_SETTINGS} is not an object"))?;
    let mut changed = false;
    for (event, entry) in claude_entries(bin) {
        let list = hooks.entry(event).or_insert_with(|| json!([]));
        let Some(list) = list.as_array_mut() else {
            anyhow::bail!("`hooks.{event}` in {CLAUDE_SETTINGS} is not an array");
        };
        let want = entry["hooks"][0]["command"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let present = list.iter().any(|e| {
            e["hooks"].as_array().is_some_and(|hs| {
                hs.iter()
                    .any(|h| h["command"].as_str().is_some_and(|c| c == want))
            })
        });
        if !present {
            list.push(entry);
            changed = true;
        }
    }
    Ok((serde_json::to_string_pretty(&root)? + "\n", changed))
}

/// Is a file at `path` ours to (re)write? Absent or marked = yes. The
/// pre-0.3.36 hand-written harna gate (`harna validate --changed`, no
/// marker) counts as ours too: upgrading it is the point.
fn ours(path: &Path) -> Option<bool> {
    let body = std::fs::read_to_string(path).ok()?;
    Some(body.contains(MARK) || (body.contains("harna") && body.contains("validate --changed")))
}

/// Write one target's file. `force` overwrites a user-owned file;
/// `craft` writes the variants with the Rust craft gates.
pub fn install(
    root: &Path,
    target: Target,
    bin: &str,
    force: bool,
    craft: bool,
) -> anyhow::Result<Outcome> {
    let path = root.join(target.path());
    let outcome = |action| Outcome {
        target,
        path: path.clone(),
        action,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = std::fs::read_to_string(&path).ok();
    let action = match target {
        Target::Claude => match merge_claude_settings(existing.as_deref(), bin) {
            Ok((_, false)) => Action::AlreadyCurrent,
            Ok((text, true)) => {
                crate::schema::store::atomic::write(&path, text)?;
                if existing.is_some() {
                    Action::Updated
                } else {
                    Action::Written
                }
            }
            Err(e) => Action::Kept(e.to_string()),
        },
        Target::Git | Target::Ci => {
            let body = if target == Target::Git {
                pre_commit_script(craft)
            } else {
                ci_workflow(craft)
            };
            match (existing.as_deref(), ours(&path)) {
                (Some(cur), _) if cur == body => Action::AlreadyCurrent,
                (Some(_), Some(false)) if !force => {
                    Action::Kept("not written by harna (rerun with force to replace it)".into())
                }
                (prev, _) => {
                    crate::schema::store::atomic::write(&path, &body)?;
                    make_executable(&path)?;
                    if prev.is_some() {
                        Action::Updated
                    } else {
                        Action::Written
                    }
                }
            }
        }
    };
    Ok(outcome(action))
}

/// Does the installed file carry the craft gates?
pub fn has_craft(root: &Path, target: Target) -> bool {
    std::fs::read_to_string(root.join(target.path()))
        .map(|s| s.contains("v1+craft"))
        .unwrap_or(false)
}

/// Is the target installed and current (either variant)?
pub fn is_current(root: &Path, target: Target, bin: &str) -> bool {
    let path = root.join(target.path());
    let Ok(cur) = std::fs::read_to_string(&path) else {
        return false;
    };
    match target {
        Target::Git => cur == pre_commit_script(false) || cur == pre_commit_script(true),
        Target::Ci => cur == ci_workflow(false) || cur == ci_workflow(true),
        Target::Claude => matches!(merge_claude_settings(Some(&cur), bin), Ok((_, false))),
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut p = std::fs::metadata(path)?.permissions();
    p.set_mode(0o755);
    std::fs::set_permissions(path, p)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "harna-hooks-{tag}-{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn claude_merge_keeps_the_users_settings_and_is_idempotent() {
        let mine = r#"{"model": "opus", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}}"#;
        let (merged, changed) = merge_claude_settings(Some(mine), "harna").unwrap();
        assert!(changed);
        let v: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(v["model"], "opus", "unrelated keys survive");
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "the user's own Stop hook stays beside ours");
        assert_eq!(
            v["hooks"]["PostToolUse"][0]["matcher"],
            "Edit|Write|MultiEdit|NotebookEdit"
        );
        assert_eq!(
            v["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "harna hook session-start"
        );
        let (_, again) = merge_claude_settings(Some(&merged), "harna").unwrap();
        assert!(!again, "a second install changes nothing");
        assert!(
            merge_claude_settings(Some("[1,2]"), "harna").is_err(),
            "not an object: not ours to rewrite"
        );
        assert!(merge_claude_settings(Some("{oops"), "harna").is_err());
    }

    #[test]
    fn a_users_own_hook_is_never_overwritten_without_force() {
        let root = dir("foreign");
        std::fs::create_dir_all(root.join(".githooks")).unwrap();
        std::fs::write(root.join(PRE_COMMIT), "#!/bin/sh\nnpm test\n").unwrap();
        let o = install(&root, Target::Git, "harna", false, false).unwrap();
        assert!(matches!(o.action, Action::Kept(_)), "{o:?}");
        assert_eq!(
            std::fs::read_to_string(root.join(PRE_COMMIT)).unwrap(),
            "#!/bin/sh\nnpm test\n"
        );
        let forced = install(&root, Target::Git, "harna", true, false).unwrap();
        assert_eq!(forced.action, Action::Updated);
        assert!(is_current(&root, Target::Git, "harna"));
    }

    #[test]
    fn the_legacy_harna_gate_is_upgraded_and_reinstall_is_a_no_op() {
        let root = dir("legacy");
        std::fs::create_dir_all(root.join(".githooks")).unwrap();
        std::fs::write(
            root.join(PRE_COMMIT),
            "#!/bin/sh\nharna validate --changed\n",
        )
        .unwrap();
        assert_eq!(
            install(&root, Target::Git, "harna", false, false)
                .unwrap()
                .action,
            Action::Updated
        );
        assert_eq!(
            install(&root, Target::Git, "harna", false, false)
                .unwrap()
                .action,
            Action::AlreadyCurrent
        );
        for t in [Target::Ci, Target::Claude] {
            assert_eq!(
                install(&root, t, "harna", false, false).unwrap().action,
                Action::Written
            );
            assert!(is_current(&root, t, "harna"), "{t:?}");
        }
    }

    #[test]
    fn craft_variants_add_fmt_clippy_and_tests_and_still_count_as_current() {
        let root = dir("craft");
        let o = install(&root, Target::Git, "harna", false, true).unwrap();
        assert_eq!(o.action, Action::Written);
        let hook = std::fs::read_to_string(root.join(PRE_COMMIT)).unwrap();
        assert!(hook.contains("pre-commit v1+craft"));
        assert!(hook.contains("cargo fmt --all -- --check"));
        assert!(hook.contains("-D warnings"));
        assert!(hook.contains("HARNA_SKIP_CRAFT"), "a named bypass");
        let gate_at = hook.find("validate --changed").unwrap();
        assert!(
            hook.find("cargo fmt").unwrap() < gate_at,
            "craft runs before conformance"
        );
        assert!(is_current(&root, Target::Git, "harna") && has_craft(&root, Target::Git));
        // Switching back to plain is an update of a harna file, not a refusal.
        assert_eq!(
            install(&root, Target::Git, "harna", false, false)
                .unwrap()
                .action,
            Action::Updated
        );
        assert!(!has_craft(&root, Target::Git));
        let ci = ci_workflow(true);
        assert!(
            ci.contains("cargo test --workspace") && ci.contains("components: rustfmt, clippy")
        );
        assert!(!ci_workflow(false).contains("cargo test"));
        // CI runs the harna that wrote it: the repo's own build, else this version.
        assert!(ci.contains("cargo install --path crates/harna-cli --locked"));
        // Not on every push: the local gate already ran these checks.
        assert!(!ci.contains("on: [push"), "{ci}");
        assert!(
            ci.contains("tags: [\"v*\"]")
                && ci.contains("pull_request:")
                && ci.contains("workflow_dispatch:")
        );
        assert!(ci.contains(&format!("--version {}", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn the_pre_commit_gate_fails_closed() {
        let s = pre_commit_script(false);
        assert!(s.contains(MARK));
        assert!(s.contains("exit 1"), "a missing binary blocks the commit");
        assert!(
            s.contains("HARNA_SKIP_HOOK"),
            "with a named, explicit bypass"
        );
        assert!(s.ends_with("exec \"$exe\" validate --changed\n"));
    }
}
