//! 1.1.4: what the owner approved stays as approved, and the changes the
//! owner wants to be asked about land with a yes — end to end.

use std::path::{Path, PathBuf};
use std::process::Command;

fn harna(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_harna"))
        // the owner's terminal, not an agent's shell
        .env_remove("CLAUDECODE")
        .env_remove("HARNA_AGENT")
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run harna")
}

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=owner",
            "-c",
            "user.email=o@o",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
        .status;
    assert!(ok.success(), "git {args:?}");
}

fn repo() -> PathBuf {
    let root = std::env::temp_dir().join(format!("harna-cli-approvals-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("web/src")).unwrap();
    std::fs::create_dir_all(root.join("db")).unwrap();
    git(&root, &["init", "-q"]);
    assert!(harna(&root, &["init"]).status.success());
    std::fs::write(
        root.join("web/src/Calendar.tsx"),
        "export const Calendar = () => null;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"react":"18"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("db/001.sql"), "create table a();\n").unwrap();
    root
}

fn findings(root: &Path) -> Vec<String> {
    let out = harna(root, &["validate", "--changed", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("pure JSON");
    v["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["check"].as_str().unwrap_or("").starts_with("approval-"))
        .map(|v| {
            format!(
                "{} {}",
                v["check"].as_str().unwrap(),
                v["file"].as_str().unwrap()
            )
        })
        .collect()
}

#[test]
fn an_approved_look_a_new_dependency_and_a_migration_each_wait_for_a_yes() {
    let root = repo();
    let ok = |args: &[&str]| {
        let o = harna(&root, args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    assert!(ok(&["approve", "calendar", "--paths", "web/src/Calendar.tsx"]).contains("calendar@1"));
    ok(&["approvals", "require", "dependency-added"]);
    ok(&[
        "approvals",
        "require",
        "path",
        "--paths",
        "db/**",
        "--reason",
        "migrations ran",
    ]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "base"]);
    assert!(findings(&root).is_empty());

    std::fs::write(
        root.join("web/src/Calendar.tsx"),
        "export const Calendar = () => 1;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"react":"18","left-pad":"1"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("db/002.sql"), "create table b();\n").unwrap();
    git(&root, &["add", "-A"]);
    assert_eq!(
        findings(&root),
        vec![
            "approval-required db/002.sql",
            "approval-required package.json",
            "approval-drift web/src/Calendar.tsx",
        ]
    );
    assert!(ok(&["approvals"]).contains("CHANGED"));

    assert!(
        ok(&["approve", "calendar"]).contains("calendar@2"),
        "paths kept"
    );
    ok(&["approve", "dep:left-pad"]);
    ok(&["approve", "migration-002", "--paths", "db/002.sql"]);
    assert!(findings(&root).is_empty());
    // the register's own change is named for the reviewer
    let out = harna(&root, &["validate", "--changed"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("approvals-changed") && text.contains("approved dep:left-pad@1"),
        "{text}"
    );
    assert!(
        !harna(&root, &["approve", "ghost"]).status.success(),
        "no paths to freeze"
    );
    assert!(
        !harna(&root, &["approve", "dep:x", "--paths", "package.json"])
            .status
            .success()
    );
    // an agent's shell is refused
    let agent = Command::new(env!("CARGO_BIN_EXE_harna"))
        .env("CLAUDECODE", "1")
        .arg("--root")
        .arg(&root)
        .args(["approve", "calendar"])
        .output()
        .unwrap();
    assert!(!agent.status.success());
    let said = String::from_utf8_lossy(&agent.stderr).to_string();
    assert!(
        said.contains("own terminal") && said.contains("harna approve calendar"),
        "{said}"
    );
    // 1.1.9: an agent may add a requirement (it only restricts), never drop one
    let as_agent = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_harna"))
            .env("CLAUDECODE", "1")
            .arg("--root")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(as_agent(&[
        "approvals",
        "require",
        "path",
        "--paths",
        "src/pay/**"
    ]));
    assert!(!as_agent(&["approvals", "revoke", "calendar"]));
    let _ = std::fs::remove_dir_all(&root);
}
