//! Component `hook-setup` (0.3.36): install and audit the enforcement
//! hooks. Files come from `generation::hooks`; this layer adds the one
//! process call git needs (`core.hooksPath`) and reports what it did.

use std::path::Path;

use crate::generation::hooks::{self, Action, Outcome, Target};

const HOOKS_DIR: &str = ".githooks";

/// What `install` did, per target, plus the git wiring.
pub struct SetupReport {
    pub outcomes: Vec<Outcome>,
    /// `core.hooksPath` result (None when git was not a target).
    pub hooks_path: Option<String>,
}

impl SetupReport {
    pub fn render(&self) -> String {
        let mut out = String::new();
        for o in &self.outcomes {
            out.push_str(&o.render());
            out.push('\n');
        }
        if let Some(h) = &self.hooks_path {
            out.push_str(h);
            out.push('\n');
        }
        out
    }
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .stdin(std::process::Stdio::null())
        .args(args)
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Install the chosen targets. `bin` is what the Claude hooks invoke;
/// `craft` adds the Rust craft gates (fmt, clippy, tests) to git and CI.
pub fn install(
    root: &Path,
    targets: &[Target],
    bin: &str,
    force: bool,
    craft: bool,
) -> anyhow::Result<SetupReport> {
    let mut outcomes = Vec::new();
    for &t in targets {
        outcomes.push(hooks::install(root, t, bin, force, craft)?);
    }
    let hooks_path = if targets.contains(&Target::Git) {
        Some(wire_hooks_path(root, force))
    } else {
        None
    };
    Ok(SetupReport {
        outcomes,
        hooks_path,
    })
}

/// Point git at `.githooks` — unless it already points somewhere else the
/// user chose (then say so, never repoint silently).
fn wire_hooks_path(root: &Path, force: bool) -> String {
    if git(root, &["rev-parse", "--git-dir"]).is_none() {
        return "git    core.hooksPath — not a git repository; the hook file is ready for when it is".into();
    }
    let current = git(root, &["config", "--local", "core.hooksPath"]).unwrap_or_default();
    if current == HOOKS_DIR {
        return format!("git    core.hooksPath = {HOOKS_DIR} — already wired");
    }
    if !current.is_empty() && !force {
        return format!(
            "git    core.hooksPath = {current} — yours, left as is; add `{HOOKS_DIR}/pre-commit` to it or rerun with force"
        );
    }
    match git(root, &["config", "--local", "core.hooksPath", HOOKS_DIR]) {
        Some(_) => format!("git    core.hooksPath = {HOOKS_DIR} — wired"),
        None => "git    core.hooksPath — `git config` failed; set it yourself: git config core.hooksPath .githooks".into(),
    }
}

/// One line per target: installed and current, or what is missing.
pub fn status(root: &Path, bin: &str) -> String {
    let mut out = String::new();
    for t in Target::ALL {
        let state = if hooks::is_current(root, t, bin) && hooks::has_craft(root, t) {
            "installed (with craft gates)"
        } else if hooks::is_current(root, t, bin) {
            "installed"
        } else if root.join(t.path()).exists() {
            "present but not harna's current version"
        } else {
            "missing"
        };
        out.push_str(&format!("{:6} {} — {state}\n", t.name(), t.path()));
    }
    let wired = git(root, &["config", "--local", "core.hooksPath"]).unwrap_or_default();
    out.push_str(&format!(
        "git    core.hooksPath = {}\n",
        if wired.is_empty() {
            "(unset — the pre-commit file is not active)".into()
        } else {
            wired
        }
    ));
    out
}

/// Did anything in the report get left for the user to resolve?
pub fn needs_attention(r: &SetupReport) -> bool {
    r.outcomes
        .iter()
        .any(|o| matches!(o.action, Action::Kept(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_outside_git_writes_files_and_says_so() {
        let root =
            std::env::temp_dir().join(format!("harna-setup-{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // File-only targets: the git target would run `git config` against
        // whatever repo encloses the temp dir — exercised end to end in the
        // CLI hook scenarios instead, inside a repo the test owns.
        let r = install(&root, &[Target::Claude, Target::Ci], "harna", false, false).unwrap();
        assert_eq!(r.outcomes.len(), 2);
        assert!(r.hooks_path.is_none(), "no git target, no git call");
        assert!(!needs_attention(&r));
        assert!(r.render().contains("installed"));
        let s = status(&root, "harna");
        assert_eq!(s.matches("— installed").count(), 2, "{s}");
        assert!(s.contains("git    .githooks/pre-commit — missing"), "{s}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
