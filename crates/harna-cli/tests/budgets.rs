//! Token budgets (0.9.4): what harna hands an agent costs context, and
//! agents run harna all day, so every agent-facing output has a ceiling.
//! Measured on harna's own repository — the daily case — through the real
//! binary. A budget that fails is a choice: trim the output, or raise the
//! ceiling here on purpose with the reason. 0.9.5 (summary first) lowered
//! them. The MCP catalog has its own budget in harna-mcp's
//! protocol tests.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// (what the agent asks, the arguments, the ceiling in bytes). About four
/// bytes make a token. Ceilings sit near today's size (0.9.5, after the
/// summary-first pass: 86 KB -> 30 KB over these eight) so the
/// outputs cannot grow by drift.
const BUDGETS: &[(&str, &[&str], usize)] = &[
    // every session pays this before its first step
    ("session briefing", &["hook", "session-start"], 7_000),
    ("status", &["status"], 1_000),
    ("command list", &["--help"], 4_500),
    ("toolbox", &["tools"], 3_000),
    ("check list", &["checks"], 3_000),
    ("place", &["place", "add a cache for the digest"], 5_500),
    ("plan", &["plan", "add a cache for the digest"], 7_500),
    ("doctor", &["doctor"], 1_500),
    // the full run: errors in full, repeated infos grouped by check (0.9.5)
    ("full validate", &["validate"], 8_000),
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bytes(root: &Path, args: &[&str]) -> usize {
    let out = Command::new(env!("CARGO_BIN_EXE_harna"))
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run harna");
    out.stdout.len() + out.stderr.len()
}

#[test]
fn agent_facing_outputs_stay_within_their_budgets() {
    let root = repo();
    if !root.join(".harna/manifest.toml").is_file() {
        return; // a packaged crate has no repository to measure
    }
    let mut over = Vec::new();
    let mut report = String::from("footprint (bytes, ~tokens / budget):\n");
    for (what, args, budget) in BUDGETS {
        let n = bytes(&root, args);
        report.push_str(&format!("  {what:<18} {n:>7} ~{:>6} / {budget}\n", n / 4));
        if n > *budget {
            over.push(format!(
                "{what} (`harna {}`): {n} > {budget}",
                args.join(" ")
            ));
        }
    }
    eprintln!("{report}");
    assert!(
        over.is_empty(),
        "over budget — trim the output, or raise its ceiling on purpose:\n  {}",
        over.join("\n  ")
    );
}
