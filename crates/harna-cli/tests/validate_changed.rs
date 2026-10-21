//! `harna validate --changed` with no changed `.rs` files must still run
//! the manifest-level gates (parity debts, data health) and keep `--json`
//! stdout parseable.

use std::path::{Path, PathBuf};
use std::process::Command;

fn harna(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_harna"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run harna")
}

fn git_repo(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("harna-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let ok = Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(ok.success(), "git init");
    root
}

#[test]
fn changed_with_no_rs_files_still_gates_on_broken_parity_ledger() {
    let root = git_repo("changed-gate");
    assert!(harna(&root, &["init"]).status.success());
    let ledger = root.join(".harna/parity");
    std::fs::create_dir_all(&ledger).unwrap();
    std::fs::write(ledger.join("conflicted.toml"), "<<<<<<< HEAD\n").unwrap();

    let out = harna(&root, &["validate", "--changed", "--json"]);
    assert!(
        !out.status.success(),
        "an unreadable ledger must fail the pre-commit gate"
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout must be pure JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    });
    assert_eq!(report["blocks"], true);
    assert!(report["violations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["check"] == "harna-data"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn changed_with_no_rs_files_on_a_healthy_repo_passes() {
    let root = git_repo("changed-clean");
    assert!(harna(&root, &["init"]).status.success());
    let out = harna(&root, &["validate", "--changed", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("pure JSON");
    assert_eq!(report["blocks"], false, "{report}");
    assert!(out.status.success());
    let _ = std::fs::remove_dir_all(&root);
}

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(ok.success(), "git {args:?}");
}

/// Review of 1.1.2 (H1): a private edit is not a public move, and a parity
/// event resolved against HEAD settles the one that is — `finish` used to
/// say "not ready" forever after either.
#[test]
fn finish_owes_no_parity_for_private_edits_or_a_resolved_event() {
    let root = git_repo("finish-parity");
    assert!(harna(&root, &["init"]).status.success());
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn shown() {}\npub fn gone() {}\nfn helper() {}\n",
    )
    .unwrap();
    assert!(harna(&root, &["scan"]).status.success());
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "base"]);

    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn shown() {}\npub fn gone() {}\nfn helper2() {}\n",
    )
    .unwrap();
    let out = String::from_utf8_lossy(&harna(&root, &["finish"]).stdout).to_string();
    assert!(out.contains("parity: nothing owed"), "private edit: {out}");

    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn shown() {}\nfn helper2() {}\n",
    )
    .unwrap();
    let out = String::from_utf8_lossy(&harna(&root, &["finish"]).stdout).to_string();
    assert!(out.contains("public surface"), "a removal is owed: {out}");
    assert!(harna(&root, &["parity", "open", "gone"]).status.success());
    let _ = harna(
        &root,
        &[
            "parity",
            "bulk",
            "gone",
            "--status",
            "ack",
            "--all-open",
            "--note",
            "checked",
        ],
    );
    let out = String::from_utf8_lossy(&harna(&root, &["finish"]).stdout).to_string();
    assert!(!out.contains("public surface"), "resolved: {out}");
    let _ = std::fs::remove_dir_all(&root);
}
