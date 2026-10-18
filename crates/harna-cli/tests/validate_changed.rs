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
    let ok = Command::new("git").arg("init").arg("-q").current_dir(&root).status().unwrap();
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
    assert!(!out.status.success(), "an unreadable ledger must fail the pre-commit gate");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout must be pure JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)));
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
