//! The CLI rule arms (moved to `commands/rules.rs` in 0.3.37): every
//! validation branch and the lifecycle, end to end through the binary.

use std::path::{Path, PathBuf};
use std::process::Command;

fn harna(root: &Path, args: &[&str]) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_harna"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run harna");
    let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    (o.status.success(), text)
}

fn repo() -> PathBuf {
    let root = std::env::temp_dir().join(format!("harna-cli-rules-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    assert!(harna(&root, &["init"]).0);
    root
}

fn add(root: &Path, id: &str, extra: &[&str]) -> (bool, String) {
    let mut args = vec!["add-rule", "--id", id, "--text", "t", "--rationale", "r"];
    args.extend_from_slice(extra);
    harna(root, &args)
}

#[test]
fn add_rule_validates_against_the_check_registry() {
    let root = repo();
    assert!(add(&root, "r1", &[]).0);
    let cases: &[(&str, &[&str], &str)] = &[
        ("r1", &[], "already exists"),
        ("r2", &["--check", "nope"], "unknown check `nope`"),
        ("r3", &["--severity", "error"], "add --check first"),
        ("r4", &["--check", "layer-boundary", "--severity", "loud"], "unknown severity `loud`"),
        ("r5", &["--pattern", "("], "not a valid regex"),
        ("r6", &["--check", "layer-boundary", "--pattern", "x"], "implies the `pattern` check"),
        ("r7", &["--limit", "3"], "--limit only names budgets"),
        ("r9", &["--check", "layer-boundary", "--limit", "3"], "one of: container-size"),
    ];
    for (id, extra, want) in cases {
        let (ok, out) = add(&root, id, extra);
        assert!(!ok && out.contains(want), "{id} {extra:?}: {out}");
    }
    assert!(add(&root, "r8", &["--check", "container-size", "--limit", "3"]).0);
    // Every budget check reads a limit — born-size and folder-size included.
    assert!(add(&root, "r10", &["--check", "born-size", "--limit", "600"]).0);
    assert!(add(&root, "r11", &["--check", "folder-size", "--limit", "8"]).0);
    let rules = std::fs::read_to_string(root.join(".harna/rules.toml")).unwrap();
    assert!(rules.contains("origin = \"user via harna CLI, "), "ISO-dated origin:\n{rules}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rule_lifecycle_round_trips() {
    let root = std::env::temp_dir().join(format!("harna-cli-rules-life-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    assert!(harna(&root, &["init"]).0);
    assert!(add(&root, "life", &[]).0);
    let (ok, out) = harna(&root, &["deprecate-rule", "life", "--reason", "superseded"]);
    assert!(ok && out.contains("deprecated"), "{out}");
    assert!(std::fs::read_to_string(root.join(".harna/rules.toml")).unwrap().contains("superseded"));
    assert!(harna(&root, &["activate-rule", "life"]).1.contains("re-activated"));
    assert!(harna(&root, &["remove-rule", "life"]).1.contains("removed"));
    let (ok, out) = harna(&root, &["remove-rule", "life"]);
    assert!(!ok && out.contains("not found"), "{out}");
    let _ = std::fs::remove_dir_all(&root);
}
