use super::*;

/// deno (0.3.46): CONTRIBUTING.md documents `./x test` and `./x spec`;
/// tools/x.ts defines test-unit, test-spec, … — and architecture docs name
/// directories that are gone.
#[test]
fn dead_paths_and_unregistered_commands_in_agent_docs_are_named() {
    let root = std::env::temp_dir().join(format!(
        "harna-docclaims-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tests/specs")).unwrap();
    std::fs::create_dir_all(root.join("runtime")).unwrap();
    std::fs::write(root.join("runtime/worker.rs"), "").unwrap();
    std::fs::write(
        root.join("CONTRIBUTING.md"),
        "Run `./x test-spec` or `./x test`.\nSpecs live in `tests/specs`, code in `runtime/worker.rs`.\n\
         Vendored code is in `third_party/`. See `docs/*.md` and `<crate>/src`.\n\
         ```\n./x spec\n```\nURLs like `https://deno.land` and flags like `--watch` are fine.\n",
    )
    .unwrap();
    let store = crate::schema::store::Store::open(&root).unwrap();
    store.init().unwrap();
    let mut cmds = crate::schema::registers::commands::Commands::default();
    for (name, run) in [
        ("test-spec", "./x test-spec"),
        ("test-unit", "./x test-unit"),
    ] {
        cmds.upsert(crate::schema::registers::commands::Command {
            name: name.into(),
            run: run.into(),
            proves: "p".into(),
            runs: "both".into(),
            scope: vec![],
        });
    }
    store.save_commands(&cmds).unwrap();

    let mut out = Vec::new();
    check(&root, &mut out);
    let msgs: Vec<String> = out
        .iter()
        .map(|v| format!("{}:{}", v.line, v.message))
        .collect();
    assert_eq!(out.len(), 3, "{msgs:?}");
    assert!(
        msgs[0].starts_with("1:`./x test` is not a registered command"),
        "{msgs:?}"
    );
    assert!(
        msgs[1].starts_with("3:`third_party/` does not exist"),
        "{msgs:?}"
    );
    assert!(
        msgs[2].starts_with("5:`./x spec` is not a registered command"),
        "{msgs:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn only_path_shaped_tokens_count() {
    for yes in ["src/lib.rs", "Cargo.toml", "tests/specs", "./x.ts"] {
        assert!(looks_like_path(yes), "{yes}");
    }
    for no in [
        "cargo test",
        "<crate>/src",
        "*.rs",
        "https://x.y/z",
        "--flag",
        "a",
        "std::fs::read",
        "x",
    ] {
        assert!(!looks_like_path(no), "{no}");
    }
}

/// Dogfood (0.3.46): harna's own AGENTS.md cites `harna parity`, `harna
/// doctor`, … — an external tool's subcommands are not the registry's to
/// enumerate; only repo-local runners are checked.
#[test]
fn external_tools_subcommands_are_not_judged() {
    let root = std::env::temp_dir().join(format!(
        "harna-docclaims-ext-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("AGENTS.md"),
        "Run `harna doctor` then `cargo build`.\n",
    )
    .unwrap();
    let store = crate::schema::store::Store::open(&root).unwrap();
    store.init().unwrap();
    let mut cmds = crate::schema::registers::commands::Commands::default();
    cmds.upsert(crate::schema::registers::commands::Command {
        name: "gate".into(),
        run: "harna validate".into(),
        proves: "p".into(),
        runs: "both".into(),
        scope: vec![],
    });
    store.save_commands(&cmds).unwrap();
    let mut out = Vec::new();
    check(&root, &mut out);
    assert!(out.is_empty(), "{out:?}");
    let _ = std::fs::remove_dir_all(&root);
}
