use super::*;
use crate::schema::model::rules::Rule;
use crate::schema::registers::commands::Command;

/// deno (0.3.46): CLAUDE.md and `.github/copilot-instructions.md` are
/// hand-kept near-copies. One block, rendered from data into every file
/// that opts in with markers; prose outside it untouched.
#[test]
fn every_opted_in_file_carries_the_same_rendered_block() {
    let root = std::env::temp_dir().join(format!(
        "harna-agents-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".github")).unwrap();
    std::fs::write(
        root.join("CLAUDE.md"),
        "# Guide\nHuman prose stays.\n<!-- harna:begin -->\nstale\n<!-- harna:end -->\nTail prose.\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".github/copilot-instructions.md"),
        "<!-- harna:begin -->\n<!-- harna:end -->\n",
    )
    .unwrap();
    std::fs::write(root.join("AGENTS.md"), "no markers: never touched\n").unwrap();

    let mut rules = Rules::default();
    rules.rules.push(Rule {
        id: "fs-through-the-trait".into(),
        text: "ext/fs reaches disk through FileSystem".into(),
        rationale: "permissions are checked there".into(),
        origin: "o".into(),
        scope: vec!["ext/fs".into()],
        check: Some("banned-api".into()),
        ..Default::default()
    });
    let mut commands = Commands::default();
    commands.upsert(Command {
        name: "test-spec".into(),
        run: "./x test-spec".into(),
        proves: "spec tests".into(),
        runs: "both".into(),
        scope: vec![],
    });
    let manifest = Manifest::default();

    let before = sync(&root, &manifest, &rules, &commands, false).unwrap();
    assert_eq!(before.len(), 2, "only files with markers: {before:?}");
    assert!(before.iter().all(|(_, stale)| *stale));
    sync(&root, &manifest, &rules, &commands, true).unwrap();
    let claude = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(claude.starts_with("# Guide\nHuman prose stays.\n"));
    assert!(claude.ends_with("Tail prose.\n"));
    assert!(claude.contains("- `fs-through-the-trait`: ext/fs reaches disk"));
    assert!(claude.contains("- `./x test-spec` — spec tests [both]"));
    let copilot = std::fs::read_to_string(root.join(".github/copilot-instructions.md")).unwrap();
    assert!(copilot.contains(&block(&manifest, &rules, &commands)));
    assert_eq!(
        std::fs::read_to_string(root.join("AGENTS.md")).unwrap(),
        "no markers: never touched\n"
    );
    let after = sync(&root, &manifest, &rules, &commands, false).unwrap();
    assert!(after.iter().all(|(_, stale)| !stale), "{after:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// zed/ruff/codex (0.3.47, H11): init must not rewrite hand-written
/// instructions. Opting in appends only the managed block; a missing file
/// gets the stock contract; a second opt-in is a no-op.
#[test]
fn opting_in_never_rewrites_prose() {
    let root = std::env::temp_dir().join(format!(
        "harna-agents-opt-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    assert_eq!(opted(&root), Opted::Missing);
    assert_eq!(opt_in(&root, "# Stock").unwrap(), Opted::Missing);
    let made = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(
        made.starts_with("# Stock\n") && made.contains(END),
        "{made}"
    );

    let prose = "# Zed\nHand-written rules.";
    std::fs::write(root.join("AGENTS.md"), prose).unwrap();
    assert_eq!(opted(&root), Opted::Hand);
    assert_eq!(opt_in(&root, "# Stock").unwrap(), Opted::Hand);
    let after = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(after.starts_with(prose), "{after}");
    assert!(!after.contains("# Stock"), "{after}");
    assert_eq!(opted(&root), Opted::Managed);
    assert_eq!(opt_in(&root, "# Stock").unwrap(), Opted::Managed);
    assert_eq!(
        std::fs::read_to_string(root.join("AGENTS.md")).unwrap(),
        after
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Review of 1.1.2: a plugin's rules are one line in the block.
#[test]
fn a_plugins_rules_are_one_line() {
    let mut rules = Rules::default();
    for id in ["own", "pack/a", "pack/b"] {
        rules.rules.push(Rule {
            id: id.into(),
            text: "t".into(),
            rationale: "r".into(),
            ..Default::default()
        });
    }
    let b = block(&Manifest::default(), &rules, &Commands::default());
    assert!(b.contains("- `own`: t"), "{b}");
    assert!(b.contains("- plugin `pack` (2 rule(s)"), "{b}");
    assert!(!b.contains("`pack/a`:"), "{b}");
}
