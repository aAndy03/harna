use super::*;

fn module(path: &str, dir: &str, src_root: &str, mods: &[&str]) -> Module {
    Module {
        path: path.into(),
        lang: "rust".into(),
        mods: mods.iter().map(|s| s.to_string()).collect(),
        crate_dir: Some(dir.into()),
        src_root: Some(src_root.into()),
        ..Default::default()
    }
}

/// zed (0.3.47, H10): `crates/agent/src/tools/evals/fixtures/*/before.rs`
/// is Rust text nothing compiles. Everything a target reaches through
/// `mod` (test-gated ones too) is source; integration tests, binaries and
/// build scripts are roots of their own.
#[test]
fn files_no_target_reaches_are_orphans() {
    let a = "crates/agent";
    let s = "crates/agent/src";
    let modules = vec![
        module("crates/agent/src/agent.rs", a, s, &["tools", "thread"]),
        module("crates/agent/src/thread.rs", a, s, &["tests"]),
        module("crates/agent/src/thread/tests.rs", a, s, &[]),
        module("crates/agent/src/tools.rs", a, s, &["edit"]),
        module("crates/agent/src/tools/edit.rs", a, s, &[]),
        module(
            "crates/agent/src/tools/evals/fixtures/case/before.rs",
            a,
            s,
            &[],
        ),
        module("crates/agent/tests/integration.rs", a, s, &["common"]),
        module("crates/agent/tests/common/mod.rs", a, s, &[]),
        module("crates/agent/src/bin/agent_cli.rs", a, s, &[]),
        module("crates/agent/build.rs", a, s, &[]),
    ];
    let orphans = orphans(&modules, &CrateIndex::default());
    assert_eq!(
        orphans.into_iter().collect::<Vec<_>>(),
        vec!["crates/agent/src/tools/evals/fixtures/case/before.rs".to_string()]
    );
}

#[test]
fn a_crate_rooted_at_lib_rs_reaches_nested_mod_rs_children() {
    let c = "crates/x";
    let s = "crates/x/src";
    let modules = vec![
        module("crates/x/src/lib.rs", c, s, &["store"]),
        module("crates/x/src/store/mod.rs", c, s, &["io"]),
        module("crates/x/src/store/io.rs", c, s, &[]),
        module("crates/x/src/stray.rs", c, s, &[]),
    ];
    let orphans = orphans(&modules, &CrateIndex::default());
    assert!(orphans.contains("crates/x/src/stray.rs"));
    assert_eq!(orphans.len(), 1, "{orphans:?}");
}

#[test]
fn a_crate_whose_roots_are_unseen_orphans_nothing() {
    let c = "crates/y";
    let s = "crates/y/src";
    let modules = vec![
        module("crates/y/src/schema/store.rs", c, s, &[]),
        module("crates/y/src/schema/migrate.rs", c, s, &[]),
    ];
    assert!(orphans(&modules, &CrateIndex::default()).is_empty());
}

/// zed (0.3.47): `src/zed.rs` is `mod zed;` of `main.rs` — named after the
/// crate, yet a module, so its children live in `src/zed/`; and a lib Cargo
/// declares at `src/server.rs` roots its own tree.
#[test]
fn a_crate_named_file_reached_by_mod_is_a_module_and_declared_targets_root() {
    let z = "crates/zed";
    let s = "crates/zed/src";
    let modules = vec![
        module("crates/zed/src/main.rs", z, s, &["zed"]),
        module("crates/zed/src/zed.rs", z, s, &["app_menus"]),
        module("crates/zed/src/zed/app_menus.rs", z, s, &[]),
        module("crates/zed/src/visual_test_runner.rs", z, s, &[]),
    ];
    let index = CrateIndex {
        targets: ["crates/zed/src/visual_test_runner.rs".to_string()].into(),
        ..CrateIndex::default()
    };
    assert!(orphans(&modules, &index).is_empty());
    // Without the declaration, the runner is data.
    let orphans = orphans(&modules, &CrateIndex::default());
    assert_eq!(orphans.len(), 1, "{orphans:?}");
}

/// zed's gpui examples: `#[path = "example_support/fonts.rs"] mod
/// example_support;` reaches the file beside the declaring one.
#[test]
fn path_attributed_mods_are_followed() {
    let g = "crates/gpui";
    let s = "crates/gpui/src";
    let modules = vec![
        module("crates/gpui/src/gpui.rs", g, s, &[]),
        module(
            "crates/gpui/examples/a11y.rs",
            g,
            s,
            &["example_support/fonts.rs"],
        ),
        module("crates/gpui/examples/example_support/fonts.rs", g, s, &[]),
        module("crates/gpui/src/settings.rs", g, s, &[]),
        module("crates/gpui/src/inner.rs", g, s, &[]),
    ];
    let o = orphans(&modules, &CrateIndex::default());
    assert!(!o.contains("crates/gpui/examples/example_support/fonts.rs"));
    assert!(o.contains("crates/gpui/src/settings.rs"));
}

/// deno's `libs/npm/fuzz` is a cargo-fuzz package no workspace lists: its
/// targets sit inside the member's folder but are not its modules — no
/// verdict, not orphans.
#[test]
fn files_of_unlisted_nested_packages_get_no_verdict() {
    let n = "libs/npm";
    let modules = vec![
        module("libs/npm/lib.rs", n, n, &[]),
        module("libs/npm/fuzz/fuzz_targets/index.rs", n, n, &[]),
        module("libs/npm/stray.rs", n, n, &[]),
    ];
    let index = CrateIndex {
        foreign: ["libs/npm/fuzz".to_string()].into(),
        ..CrateIndex::default()
    };
    let o = orphans(&modules, &index);
    assert_eq!(
        o.into_iter().collect::<Vec<_>>(),
        vec!["libs/npm/stray.rs".to_string()]
    );
}
