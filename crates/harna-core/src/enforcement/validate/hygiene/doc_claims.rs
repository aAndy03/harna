//! `doc-claims` (F1): what agent and contributor docs claim must be true.
//! deno's CONTRIBUTING.md documents `./x test` and `./x spec`, which its
//! task runner does not define; doc/architecture.md lists `third_party/`
//! and `coverage/`, which do not exist. An agent following those docs
//! loses its first minutes to dead ends.
//!
//! Read: AGENTS.md, CLAUDE.md, CONTRIBUTING.md (root and .github/) and
//! .github/copilot-instructions.md. Two claims are checked:
//! - a backticked repo path (has a `/` or a known file extension, no
//!   spaces, globs or placeholders) must exist;
//! - a backticked invocation whose runner the command registry knows
//!   is repo-local (`./x …`, a script in the tree) must name a registered
//!   subcommand — register every task of a runner the docs cite.
//!
//! Advisory by default: docs drift is a finding, not a gate.

use std::path::Path;

use super::{finding, Violation};

const DOCS: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "CONTRIBUTING.md",
    ".github/CONTRIBUTING.md",
    ".github/copilot-instructions.md",
];
const EXTS: &[&str] = &[
    ".rs", ".toml", ".md", ".json", ".jsonc", ".ts", ".js", ".yml", ".yaml", ".sh",
];

pub fn check(root: &Path, out: &mut Vec<Violation>) {
    let store = crate::schema::store::Store::open(root).ok();
    // a plugin rule id (`gpui-kit/stable-identity`) names a rule, not a
    // path (0.5.9 field test: the generated AGENTS block tripped 12 claims)
    let plugins: Vec<String> = store
        .as_ref()
        .map(|s| {
            s.load_plugins_lenient()
                .0
                .into_iter()
                .map(|p| p.meta.name)
                .collect()
        })
        .unwrap_or_default();
    let runners = store
        .and_then(|s| s.load_commands().ok())
        .map(|c| c.runners())
        .unwrap_or_default();
    for doc in DOCS {
        let Ok(text) = std::fs::read_to_string(root.join(doc)) else {
            continue;
        };
        let mut in_fence = false;
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            let spans: Vec<&str> = if in_fence {
                vec![line.trim()]
            } else {
                line.split('`').skip(1).step_by(2).collect()
            };
            for span in spans {
                let rule_id = span
                    .split_once('/')
                    .is_some_and(|(p, rest)| plugins.iter().any(|n| n == p) && !rest.contains('/'));
                if rule_id {
                    continue;
                }
                if let Some(msg) = claim_problem(root, span, &runners) {
                    out.push(finding(
                        "doc-claims",
                        doc,
                        i + 1,
                        msg,
                        "fix the doc or the tree — agents act on what it says".into(),
                    ));
                }
            }
        }
    }
}

fn claim_problem(root: &Path, span: &str, runners: &[(String, Vec<String>)]) -> Option<String> {
    let span = span.trim();
    let mut words = span.split_whitespace();
    let first = words.next()?;
    // Only repo-local runners (`./x`, a script in the tree) are checked:
    // their task list is the repo's own, so the registry can hold all of
    // it. External tools (cargo, harna) have subcommands nobody registers.
    let local = first.starts_with("./") || root.join(first).is_file();
    if let Some((_, subs)) = runners.iter().find(|(r, _)| r == first).filter(|_| local) {
        let sub = words.next()?;
        if !sub.starts_with('-') && !subs.iter().any(|s| s == sub) {
            return Some(format!(
                "`{first} {sub}` is not a registered command (known: {})",
                subs.join(", ")
            ));
        }
        return None;
    }
    if span.contains(char::is_whitespace) || !looks_like_path(span) {
        return None;
    }
    let p = span.trim_start_matches("./").trim_end_matches('/');
    if root.join(p).exists() || ignored(root, p) {
        return None;
    }
    // harna's own registers (`.harna/roadmap.toml`, `.harna/terms.toml`)
    // appear on first use; the stock AGENTS.md names them before then
    // (0.3.49: `harna init --agents` then tripped its own check).
    if p.starts_with(".harna/") && root.join(".harna").is_dir() {
        return None;
    }
    Some(format!("`{span}` does not exist in the repository"))
}

/// A path git ignores (`target/debug/deno`) is a build output the docs may
/// name without it being in the tree.
fn ignored(root: &Path, p: &str) -> bool {
    std::process::Command::new("git")
        .args(["check-ignore", "-q", p])
        .current_dir(root)
        .status()
        .is_ok_and(|s| s.success())
}

fn looks_like_path(s: &str) -> bool {
    let banned = [
        '*', '<', '>', '{', '}', '…', '$', '|', '(', ')', ':', '=', '@', '#', '?', '"', '\'',
    ];
    if s.is_empty() || s.chars().any(|c| banned.contains(&c)) || s.contains("..") {
        return false;
    }
    if s.starts_with("http") || s.starts_with('-') || s.starts_with('/') {
        return false;
    }
    // `dir/x`, or `dir/` — a trailing slash is how docs name a folder.
    let segs = s.split('/').filter(|x| !x.is_empty()).count();
    let has_dir = (s.contains('/') && segs >= 2) || (s.ends_with('/') && segs == 1);
    has_dir || EXTS.iter().any(|e| s.ends_with(e) && s.len() > e.len())
}

#[cfg(test)]
mod tests;
