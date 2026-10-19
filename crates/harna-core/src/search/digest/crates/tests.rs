use super::*;

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn repo(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "harna-crates-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

/// The deno shape (0.3.44): `lib.rs` at the crate root, dir ≠ package
/// (`ext/fs` = `deno_fs`), a `package =` rename (`v8` -> `deno_v8`), a
/// crate reachable only as a workspace path dependency (implicit member),
/// and one conventional `src/` crate beside them.
#[test]
fn cargo_declarations_not_folder_conventions_define_crates() {
    let root = repo("deno");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"cli\", \"ext/*\", \"libs/parser\"]\n\
         [workspace.dependencies]\n\
         deno_fs = { version = \"0.1\", path = \"./ext/fs\" }\n\
         deno_os = { path = \"./ext/os\" }\n\
         v8 = { package = \"deno_v8\", path = \"./libs/deno_v8\" }\n",
    );
    write(
        &root,
        "cli/Cargo.toml",
        "[package]\nname = \"deno\"\n[lib]\nname = \"deno_lib\"\npath = \"lib.rs\"\n\
         [[bin]]\nname = \"deno\"\npath = \"main.rs\"\n",
    );
    write(
        &root,
        "ext/fs/Cargo.toml",
        "[package]\nname = \"deno_fs\"\n[lib]\npath = \"lib.rs\"\n",
    );
    write(
        &root,
        "ext/net/Cargo.toml",
        "[package]\nname = \"deno_net\"\n[lib]\npath = \"lib.rs\"\n",
    );
    write(
        &root,
        "ext/os/Cargo.toml",
        "[package]\nname = \"deno_os\"\n[lib]\npath = \"lib.rs\"\n",
    );
    write(
        &root,
        "libs/deno_v8/Cargo.toml",
        "[package]\nname = \"deno_v8\"\n[lib]\npath = \"lib.rs\"\n",
    );
    write(
        &root,
        "libs/parser/Cargo.toml",
        "[package]\nname = \"cli-parser\"\n",
    );
    write(&root, "libs/parser/src/lib.rs", "");

    let idx = CrateIndex::load(&root);
    let dirs: Vec<&str> = idx.crates.iter().map(|c| c.dir.as_str()).collect();
    for d in [
        "cli",
        "ext/fs",
        "ext/net",
        "ext/os",
        "libs/deno_v8",
        "libs/parser",
    ] {
        assert!(dirs.contains(&d), "{d} missing: {dirs:?}");
    }

    let (c, stem) = idx.locate("ext/fs/ops.rs").unwrap();
    assert_eq!((c.package.as_str(), stem.as_str()), ("deno_fs", "ops"));
    let (c, stem) = idx.locate("ext/net/quic/mod.rs").unwrap();
    assert_eq!((c.dir.as_str(), stem.as_str()), ("ext/net", "quic"));
    let (c, stem) = idx.locate("libs/parser/src/convert.rs").unwrap();
    assert_eq!(
        (c.src_root.as_str(), stem.as_str()),
        ("libs/parser/src", "convert")
    );
    let (c, stem) = idx.locate("libs/parser/tests/e2e.rs").unwrap();
    assert_eq!(
        (c.dir.as_str(), stem.as_str()),
        ("libs/parser", "tests/e2e")
    );
    assert!(idx.locate("tools/x.rs").is_none(), "no crate hosts it");

    // Import names: package, lib name, rename key — all in `use` spelling.
    for (import, dir) in [
        ("deno_fs", "ext/fs"),
        ("deno_lib", "cli"),
        ("deno", "cli"),
        ("v8", "libs/deno_v8"),
        ("cli_parser", "libs/parser"),
        ("deno_os", "ext/os"),
    ] {
        assert_eq!(idx.dir_of(import), Some(dir), "{import}");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_root_package_and_nested_crates_resolve_to_the_deepest_owner() {
    let root = repo("nested");
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"app\"\n[workspace]\nmembers = [\"crates/*\"]\n",
    );
    write(&root, "src/main.rs", "");
    write(
        &root,
        "crates/core/Cargo.toml",
        "[package]\nname = \"app-core\"\n",
    );
    write(&root, "crates/core/src/lib.rs", "");
    let idx = CrateIndex::load(&root);
    let (c, stem) = idx.locate("crates/core/src/store.rs").unwrap();
    assert_eq!((c.package.as_str(), stem.as_str()), ("app-core", "store"));
    let (c, stem) = idx.locate("src/cli/args.rs").unwrap();
    assert_eq!((c.dir.as_str(), stem.as_str()), (".", "cli/args"));
    assert_eq!(idx.dir_of("app_core"), Some("crates/core"));
    let _ = fs::remove_dir_all(&root);
}

/// deno (0.3.44): `use deno_telemetry::OtelConfig` names an item at the
/// crate root; no module stem matches, so the edge must land on lib.rs —
/// before, it fell off the map and adopt's depends_on missed it.
#[test]
fn an_import_of_a_crate_root_item_lands_on_its_lib() {
    use crate::search::digest::{hosted_module, resolve_dep, Digest, Module};
    let module = |path: &str, dir: &str, src: &str| Module {
        path: path.into(),
        lang: "rust".into(),
        signatures: vec![],
        deps: vec![],
        body_hash: String::new(),
        inventory: vec![],
        mods: vec![],
        crate_dir: Some(dir.into()),
        src_root: Some(src.into()),
    };
    let d = Digest {
        generated_at: "now".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![
            module("ext/telemetry/lib.rs", "ext/telemetry", "ext/telemetry"),
            module("ext/telemetry/otel.rs", "ext/telemetry", "ext/telemetry"),
        ],
    };
    let names = vec!["deno_telemetry".to_string()];
    let dirs: BTreeMap<String, String> =
        [("deno_telemetry".to_string(), "ext/telemetry".to_string())].into();
    let hit = |dep: &str| {
        let t = resolve_dep(dep, "args/flags", &names).unwrap();
        hosted_module(&d, &dirs, Some("cli"), &t).map(|m| m.path.as_str())
    };
    assert_eq!(
        hit("deno_telemetry::otel::Span"),
        Some("ext/telemetry/otel.rs")
    );
    assert_eq!(
        hit("deno_telemetry::OtelConfig"),
        Some("ext/telemetry/lib.rs")
    );
}
