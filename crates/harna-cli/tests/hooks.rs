//! Human–agent scenarios for the enforcement hooks (0.3.36), run against
//! the real `harna` binary in real git repos. Each test is one story:
//! what the human or agent does, and what the hooks make happen — without
//! anyone remembering to run a gate.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn harna(root: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_harna"))
        .arg("--root")
        .arg(root)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run harna");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn git(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
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
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A git repo with harna initialized and a clean committed crate.
fn repo(tag: &str) -> Option<PathBuf> {
    let root = std::env::temp_dir().join(format!("harna-hookstory-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    if !git(&root, &["init", "-q"]) {
        return None; // no git on this machine
    }
    assert!(harna(&root, &["init"], "").status.success());
    std::fs::write(root.join("src/lib.rs"), "pub fn ok() -> u32 { 1 }\n").unwrap();
    assert!(git(&root, &["add", "-A"]) && git(&root, &["commit", "-q", "-m", "clean"]));
    Some(root)
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The agent hard-codes a URL. It hears about it from the hook right
/// after the edit — not at commit time, not in review — and once it fixes
/// the file, the hook goes quiet again.
#[test]
fn a_bad_edit_is_reported_while_it_is_fresh() {
    let Some(root) = repo("post-edit") else {
        return;
    };
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn url() -> &'static str { \"https://api.example.com\" }\n",
    )
    .unwrap();
    let o = harna(&root, &["hook", "post-edit"], "{}");
    assert_eq!(
        o.status.code(),
        Some(2),
        "exit 2 feeds stderr back to the agent"
    );
    assert!(err(&o).contains("no-hardcoded-urls"), "{}", err(&o));
    assert!(err(&o).contains("src/lib.rs"));

    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn url(base: &str) -> String { format!(\"{base}/v1\") }\n",
    )
    .unwrap();
    let o = harna(&root, &["hook", "post-edit"], "{}");
    assert_eq!(o.status.code(), Some(0));
    assert!(
        err(&o).is_empty() && o.stdout.is_empty(),
        "a conforming edit is silent"
    );
}

/// The agent says "done" with the violation still in place. The stop hook
/// sends it back once with the reason; if it tries to stop again, harna
/// lets it — but makes it tell the user — instead of looping forever.
#[test]
fn a_blocked_change_sends_the_agent_back_once() {
    let Some(root) = repo("stop") else { return };
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn url() -> &'static str { \"https://api.example.com\" }\n",
    )
    .unwrap();
    let first = harna(&root, &["hook", "stop"], r#"{"stop_hook_active": false}"#);
    assert_eq!(first.status.code(), Some(2));
    assert!(err(&first).contains("BLOCKED"), "{}", err(&first));
    let retry = harna(&root, &["hook", "stop"], r#"{"stop_hook_active": true}"#);
    assert_eq!(retry.status.code(), Some(0), "no infinite loop");
    assert!(err(&retry).contains("tell the user"), "{}", err(&retry));
}

/// A new session opens. The agent was never told to call onboarding —
/// the session-start hook briefs it anyway.
#[test]
fn every_session_starts_briefed() {
    let Some(root) = repo("session") else { return };
    let o = harna(&root, &["hook", "session-start"], "{}");
    assert_eq!(o.status.code(), Some(0));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("concepts: harna's vocabulary"), "{out}");
}

/// The human runs `harna hooks install` in a repo that already has their
/// own Claude settings and their own pre-commit hook. harna adds itself
/// beside the settings, leaves the hook alone and says so, and a second
/// install changes nothing.
#[test]
fn installing_respects_what_the_user_already_has() {
    let Some(root) = repo("install") else { return };
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(
        root.join(".claude/settings.json"),
        r#"{"permissions": {"allow": ["Bash(cargo test)"]}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join(".githooks")).unwrap();
    std::fs::write(
        root.join(".githooks/pre-commit"),
        "#!/bin/sh\nnpm run lint\n",
    )
    .unwrap();

    let o = harna(&root, &["hooks", "install"], "");
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(
        out.contains("git    .githooks/pre-commit — left untouched"),
        "{out}"
    );
    assert!(
        out.contains("claude .claude/settings.json — updated"),
        "{out}"
    );
    assert!(
        out.contains("ci     .github/workflows/harna.yml — installed"),
        "{out}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".githooks/pre-commit")).unwrap(),
        "#!/bin/sh\nnpm run lint\n"
    );
    let settings = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap();
    assert!(
        settings.contains("Bash(cargo test)"),
        "the user's permissions survive"
    );
    assert!(settings.contains("harna hook post-edit"));

    let again =
        String::from_utf8_lossy(&harna(&root, &["hooks", "install"], "").stdout).into_owned();
    assert!(
        again.contains("claude .claude/settings.json — already current"),
        "{again}"
    );
    assert!(
        again.contains("ci     .github/workflows/harna.yml — already current"),
        "{again}"
    );
}

/// Fresh repo, the human installs the git hook and commits a hard-coded
/// URL: the commit is refused. With harna missing from the machine the
/// hook refuses too (fail closed) — until the human explicitly bypasses.
#[test]
fn the_commit_gate_holds_even_without_harna_installed() {
    let Some(root) = repo("commit") else { return };
    if Command::new("sh")
        .arg("-c")
        .arg("true")
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        return; // no POSIX shell to run git hooks with
    }
    let o = harna(&root, &["hooks", "install", "--target", "git"], "");
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.contains("core.hooksPath = .githooks — wired"), "{out}");

    let bin = env!("CARGO_BIN_EXE_harna");
    let commit = |envs: &[(&str, &str)]| {
        Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "x",
            ])
            .current_dir(&root)
            .envs(envs.iter().copied())
            .output()
            .unwrap()
    };
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn url() -> &'static str { \"https://api.example.com\" }\n",
    )
    .unwrap();
    assert!(git(&root, &["add", "-A"]));
    let blocked = commit(&[("HARNA_BIN", bin)]);
    assert!(!blocked.status.success(), "a violating commit is refused");

    // harna not installed anywhere the hook looks: still refused. Run the
    // hook itself with a PATH holding only the shell's own directory.
    let sh_dir = Command::new("sh")
        .args(["-c", "dirname \"$(command -v sh)\""])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let missing = Command::new("sh")
        .arg(".githooks/pre-commit")
        .current_dir(&root)
        .env("HARNA_BIN", "")
        .env("PATH", &sh_dir)
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1), "fails closed without harna");
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("cannot run"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );

    // The human decides to bypass, explicitly and visibly.
    let bypass = commit(&[("HARNA_SKIP_HOOK", "1")]);
    assert!(
        bypass.status.success(),
        "{}",
        String::from_utf8_lossy(&bypass.stderr)
    );
    assert!(String::from_utf8_lossy(&bypass.stderr).contains("skipped (HARNA_SKIP_HOOK=1)"));
}
