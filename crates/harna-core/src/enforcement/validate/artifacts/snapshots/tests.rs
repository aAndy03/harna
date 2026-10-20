use super::*;
use crate::search::digest::Module;

fn module(path: &str, dir: &str) -> Module {
    Module {
        path: path.into(),
        lang: "rust".into(),
        crate_dir: Some(dir.into()),
        src_root: Some(format!("{dir}/src")),
        ..Default::default()
    }
}

/// ruff (0.3.48, R5): `ruff_linter__rules__pyflakes__tests__F401_0.py.snap`
/// came from `ruff_linter::rules::pyflakes::tests` — an inline `mod tests`
/// of `rules/pyflakes/mod.rs`. Snapshots of a deleted module are named;
/// dynamic snapshot names and non-insta files get no verdict.
#[test]
fn snapshots_whose_module_is_gone_are_named() {
    let root = std::env::temp_dir().join(format!(
        "harna-snapshots-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    let c = "crates/ruff_linter";
    w("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    w(
        &format!("{c}/Cargo.toml"),
        "[package]\nname = \"ruff_linter\"\n",
    );
    w(&format!("{c}/src/lib.rs"), "pub mod rules;\n");
    w(&format!("{c}/src/rules/mod.rs"), "pub mod pyflakes;\n");
    w(
        &format!("{c}/src/rules/pyflakes/mod.rs"),
        "pub fn f() {}\n#[cfg(test)]\nmod tests {\n}\n",
    );
    let snaps = format!("{c}/src/rules/pyflakes/snapshots");
    w(
        &format!("{snaps}/ruff_linter__rules__pyflakes__tests__F401_0.py.snap"),
        "---\n",
    );
    w(
        &format!("{snaps}/ruff_linter__rules__pycodestyle__tests__E501.snap"),
        "---\n",
    );
    w(&format!("{snaps}/custom_name.snap"), "---\n");
    let digest = Digest {
        generated_at: String::new(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![
            module(&format!("{c}/src/lib.rs"), c),
            module(&format!("{c}/src/rules/mod.rs"), c),
            module(&format!("{c}/src/rules/pyflakes/mod.rs"), c),
        ],
        orphans: vec![],
    };
    let mut out = Vec::new();
    check(&root, &Rules::default(), None, Some(&digest), &mut out);
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0]
        .file
        .ends_with("ruff_linter__rules__pycodestyle__tests__E501.snap"));
    assert!(
        out[0]
            .message
            .contains("`ruff_linter::rules::pycodestyle::tests`"),
        "{}",
        out[0].message
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The first pass on ruff and codex misread three shapes: `lib.rs` and
/// `main.rs` both root the crate (ruff_python_formatter's `mod tests` is
/// in lib.rs), `#[path = "…"] pub(super) mod tests;` (codex tui), and a
/// snapshot named explicitly with a module-like prefix (codex core).
#[test]
fn crate_roots_path_mods_and_explicit_names_are_referenced() {
    let root = std::env::temp_dir().join(format!(
        "harna-snapshots-ref-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    let c = "crates/fmt";
    w("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    w(&format!("{c}/Cargo.toml"), "[package]\nname = \"fmt\"\n");
    w(
        &format!("{c}/src/lib.rs"),
        "pub mod app;\n#[cfg(test)]\nmod tests {}\n",
    );
    w(&format!("{c}/src/main.rs"), "fn main() {}\n");
    w(
        &format!("{c}/src/app.rs"),
        "#[path = \"app_tests.rs\"]\npub(super) mod tests;\n",
    );
    w(
        &format!("{c}/src/session/tests.rs"),
        "insta::assert_snapshot!(\"fmt__codex_tests__fork\", x);\n",
    );
    for s in [
        "src/snapshots/fmt__tests__strings.snap",
        "src/snapshots/fmt__app__tests__actions__archive.snap",
        "src/session/snapshots/fmt__codex_tests__fork.snap",
    ] {
        w(&format!("{c}/{s}"), "---\n");
    }
    let digest = Digest {
        generated_at: String::new(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![
            module(&format!("{c}/src/lib.rs"), c),
            module(&format!("{c}/src/main.rs"), c),
            module(&format!("{c}/src/app.rs"), c),
            module(&format!("{c}/src/session/tests.rs"), c),
        ],
        orphans: vec![],
    };
    let mut out = Vec::new();
    check(&root, &Rules::default(), None, Some(&digest), &mut out);
    assert!(out.is_empty(), "{out:?}");
    let _ = std::fs::remove_dir_all(&root);
}
