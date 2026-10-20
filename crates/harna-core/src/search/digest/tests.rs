use super::*;

#[test]
fn extract_deps_internal_only() {
    let src = "\
use std::fs;
use serde::Serialize;
use crate::schema::store::Store;
use crate::schema::store::{Store, StoreOpen};
use super::digest::Digest;
use self::helpers::x;
use harna_core::schema::manifest::Manifest;
pub fn main() {}
";
    let names = vec!["harna_core".to_string()];
    let deps = extract_deps(src, &names, &[]);
    assert!(deps.contains(&"crate::schema::store::Store".to_string()));
    // a brace group expands to its items (0.3.46: it used to collapse to
    // its head, so `use client::{proto}` recorded no dependency at all)
    assert!(deps.contains(&"crate::schema::store::StoreOpen".to_string()));
    assert!(deps.contains(&"super::digest::Digest".to_string()));
    assert!(deps.contains(&"self::helpers::x".to_string()));
    assert!(deps.contains(&"harna_core::schema::manifest::Manifest".to_string()));
    // std and external crates are not internal deps
    assert!(!deps.contains(&"std::fs".to_string()));
    assert!(!deps.contains(&"serde::Serialize".to_string()));
}

#[test]
fn extract_deps_inline_references_count() {
    let src = "\
fn run(store: &crate::schema::store::Store) -> u32 {
harna_core::schema::manifest::count(store)
}
let url = fetch(\"https://example.com\");
// crate::docs::not_a_dep
";
    let names = vec!["harna_core".to_string()];
    let deps = extract_deps(src, &names, &[]);
    assert!(deps.contains(&"crate::schema::store::Store".to_string()));
    assert!(deps.contains(&"harna_core::schema::manifest::count".to_string()));
    // comment references are not deps
    assert!(!deps.contains(&"crate::docs::not_a_dep".to_string()));
}

#[test]
fn extract_deps_bare_sibling_references_count() {
    let src = "\
use crate::schema::store::Store;
mod rules {
pub fn r() {}
}
pub mod payloads;
fn go() { rules::r(); payloads::walk(); }
use rules::ghost;
";
    let deps = extract_deps(src, &["harna_core".to_string()], &declared_mods(src));
    assert!(
        deps.contains(&"self::rules::r".to_string()),
        "sibling call site must become an edge"
    );
    assert!(deps.contains(&"self::payloads::walk".to_string()));
    // `use` statements cannot be bare sibling paths in Rust 2018+ —
    // the exemption keeps that channel from inventing edges.
    assert!(!deps.contains(&"self::rules::ghost".to_string()));
}

#[test]
fn extract_deps_skips_test_sections() {
    let src = "use crate::schema::store::Store;\n\
#[cfg(test)]\nmod tests {\n    use crate::fixture::OnlyFixture;\n}";
    let deps = extract_deps(src, &[], &[]);
    assert!(deps.contains(&"crate::schema::store::Store".to_string()));
    assert!(!deps.contains(&"crate::fixture::OnlyFixture".to_string()));
}

#[test]
fn scan_respects_gitignore_and_falls_back_without_git() {
    let root = std::env::temp_dir().join(format!("harna-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("generated")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    std::fs::write(root.join("src/real.rs"), "pub fn real() {}").unwrap();
    std::fs::write(root.join("generated/noise.rs"), "pub fn noise() {}").unwrap();

    // With git: generated/ is ignored -> invisible to the map.
    std::fs::write(root.join(".gitignore"), "/generated/\n").unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .output()
        .unwrap();
    let d = build(&root, 100).unwrap();
    assert!(d.modules.iter().any(|m| m.path.contains("real.rs")));
    assert!(
        !d.modules.iter().any(|m| m.path.contains("noise.rs")),
        ".gitignore must rule the scan"
    );

    // Without git: fallback walk (same scope as the old skip-list behavior).
    std::fs::remove_dir_all(root.join(".git")).unwrap();
    let d2 = build(&root, 100).unwrap();
    assert!(d2.modules.iter().any(|m| m.path.contains("noise.rs")));
}

#[test]
fn incremental_scan_reuses_unchanged_and_rescans_changed() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("harna-incr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    // Both files predate the first scan: their mtimes are older than the
    // baseline stamp, so one full pass is enough to set up the reuse test.
    std::fs::write(root.join("src/stable.rs"), "pub fn stable() {}").unwrap();
    std::fs::write(root.join("src/moving.rs"), "pub fn first() {}").unwrap();

    let (first, reused0, scanned0) = build_incremental(&root, 100, None, false)?;
    assert_eq!(reused0, 0);
    assert_eq!(scanned0, 2);

    // Baseline is stamped "now"; changing moving.rs after the scan moves
    // its mtime past the stamp -> it must be rescanned. stable.rs stays.
    std::fs::write(root.join("src/moving.rs"), "pub fn second() {}").unwrap();

    let (second, reused, scanned) = build_incremental(&root, 100, Some(&first), false)?;
    assert!(
        reused == 1,
        "stable.rs reused, got reused={reused} scanned={scanned}"
    );
    assert_eq!(second.extractor_version, EXTRACTOR_VERSION);
    second
        .modules
        .iter()
        .find(|m| m.path.ends_with("moving.rs"))
        .filter(|m| m.signatures.iter().any(|s| s.contains("second")))
        .expect("changed module must be rescanned");
    second
        .modules
        .iter()
        .find(|m| m.path.ends_with("stable.rs"))
        .filter(|m| m.signatures.iter().any(|s| s.contains("stable")))
        .expect("reused or rescanned, content must be intact");

    // Hash-verified reuse is deterministic: stable reused, moving rescanned.
    assert_eq!(reused, 1);
    assert_eq!(scanned, 1);
    assert_eq!(reused + scanned, second.modules.len());

    // Full rebuild ignores reuse entirely and still agrees.
    let (third, _, _) = build_incremental(&root, 100, Some(&second), true)?;
    assert_eq!(third.modules.len(), second.modules.len());
    assert!(third
        .modules
        .iter()
        .any(|m| m.signatures.iter().any(|s| s.contains("second"))));
    Ok(())
}

#[test]
fn extractor_upgrade_forces_a_rebuild() -> anyhow::Result<()> {
    // A map whose extractor_version belongs to an older harna must not
    // be reused module by module — the file hashes cannot know that the
    // extractor gained new dep shapes.
    let root = std::env::temp_dir().join(format!("harna-extr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    std::fs::write(root.join("src/a.rs"), "pub fn a() {}").unwrap();
    let (fresh, reused0, _) = build_incremental(&root, 100, None, false)?;
    assert_eq!(reused0, 0);

    let mut stale = fresh.clone();
    stale.extractor_version = 0;
    let (rebuilt, reused, _) = build_incremental(&root, 100, Some(&stale), false)?;
    assert_eq!(reused, 0, "stale-extractor maps rebuild in full");
    assert_eq!(rebuilt.extractor_version, EXTRACTOR_VERSION);
    Ok(())
}

#[test]
fn parallel_scan_matches_the_sequential_result() -> anyhow::Result<()> {
    // Enough files to cross the MIN_FILES_PER_THREAD threshold: the
    // chunked threads must produce exactly the sequential digest —
    // same module set, same reuse counts (hash-verified).
    let root = std::env::temp_dir().join(format!("harna-par-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    let n = 24;
    for i in 0..n {
        std::fs::write(
            root.join(format!("src/m{i}.rs")),
            format!("pub fn f{i}() {{}}\n"),
        )?;
    }
    let (first, reused0, scanned0) = build_incremental(&root, 100, None, false)?;
    assert_eq!(reused0, 0);
    assert_eq!(scanned0, n);
    assert_eq!(first.modules.len(), n);
    let paths: Vec<&str> = first.modules.iter().map(|m| m.path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(
        paths, sorted,
        "digest order holds through the parallel chunks"
    );

    // Second pass: every body unchanged, all reused, module-for-module
    // identical — whichever thread parsed what.
    let (second, reused, scanned) = build_incremental(&root, 100, Some(&first), false)?;
    assert_eq!(reused, n);
    assert_eq!(scanned, 0);
    assert_eq!(
        second
            .modules
            .iter()
            .map(|m| m.path.as_str())
            .collect::<Vec<_>>(),
        paths,
    );
    assert_eq!(second.signature_count(), first.signature_count());
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn reusable_names_are_hash_verified_and_equal_declared_names() {
    let module = Module {
        path: "src/a.rs".into(),
        lang: "rust".into(),
        inventory: vec![
            Item {
                vis: "pub".into(),
                kind: "struct".into(),
                name: "Store".into(),
                text: "pub struct Store".into(),
            },
            Item {
                vis: "priv".into(),
                kind: "fn".into(),
                name: "seal".into(),
                text: "fn seal() {}".into(),
            },
            // Duplicate name (impl + struct): declared_names dedups.
            Item {
                vis: "priv".into(),
                kind: "impl".into(),
                name: "Store".into(),
                text: "impl Store {}".into(),
            },
        ],
        ..Default::default()
    };
    let src = "pub struct Store;\nfn seal() {}\nimpl Store {}\n";

    // No hash recorded: no proof, no reuse.
    assert!(reusable_declared_names(&module, src).is_none());

    // Hash of a DIFFERENT body: no proof, no reuse (the live file may
    // have declared something new).
    let mut stale = module.clone();
    stale.body_hash = hash_body(b"pub struct Old;");
    assert!(reusable_declared_names(&stale, src).is_none());

    // The recorded hash of exactly this body: the inventory IS the
    // parse — sorted, deduped, identical to declared_names.
    let mut proven = module;
    proven.body_hash = hash_body(src.as_bytes());
    let names = reusable_declared_names(&proven, src).expect("hash proves the catalog");
    assert_eq!(names, vec!["Store".to_string(), "seal".to_string()]);
    assert_eq!(names, declared_names(src), "the catalog equals the parse");
}

#[test]
fn test_fixtures_do_not_pollute_signatures() {
    // scan_rust_file path exercised indirectly: build() on a temp crate.
    let root = std::env::temp_dir().join(format!("harna-sigfix-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/m.rs"),
        "pub fn real() {}\n\n#[cfg(test)]\nmod tests {\n    \
pub fn phantom() {}\n}",
    )
    .unwrap();
    let d = build(&root, 100).unwrap();
    let m = d.modules.iter().find(|m| m.path == "src/m.rs").unwrap();
    assert_eq!(m.signatures, vec!["pub fn real() {}".to_string()]);
}

#[test]
fn renders_overview_with_truncation() {
    let d = Digest {
        orphans: Vec::new(),
        generated_at: "now".into(),
        extractor_version: EXTRACTOR_VERSION,
        modules: vec![Module {
            path: "src/lib.rs".into(),
            lang: "rust".into(),
            signatures: (0..9).map(|i| format!("pub fn f{i}()")).collect(),
            ..Default::default()
        }],
    };
    let text = d.render_overview(5);
    assert!(text.contains("src/lib.rs"));
    assert!(text.contains("+4 more")); // 9 sigs, first 5 shown
                                       // The delegation (0.3.16): render_overview == render_overview_from
                                       // over the leading slice — one renderer, two read shapes.
    assert_eq!(text, render_overview_from("now", 1, 9, &d.modules[..1], 5));

    let tree = d.render_tree();
    assert!(tree.contains("lib.rs  (9 pub, 0 items)"));
}

#[test]
fn parse_pub_crate() {
    use super::*;
    let it = Item::parse("pub(crate) fn crate_visible() {}");
    println!("PARSE: {it:?}");
    assert!(it.is_some());
}

#[test]
fn inventory_captures_visibility_and_kinds() {
    let root = std::env::temp_dir().join(format!("harna-inv-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/m.rs"),
        "pub struct Api;
fn helper() {}
pub(crate) fn crate_visible() {}
impl Default for Api {
fn default() -> Self { Self }
}
",
    )
    .unwrap();
    let d = build(&root, 10).unwrap();
    let m = d.modules.iter().find(|m| m.path == "src/m.rs").unwrap();
    for i in &m.inventory {
        println!("INV {} {} {}", i.vis, i.kind, i.name);
    }
    let kinds: Vec<(&str, &str)> = m
        .inventory
        .iter()
        .map(|i| (i.kind.as_str(), i.vis.as_str()))
        .collect();
    assert!(kinds.contains(&("impl", "priv")));
    assert!(kinds.contains(&("fn", "priv")), "private fn in inventory");
    assert!(
        kinds.contains(&("fn", "pub")),
        "pub(crate) counts as pub boundary"
    );
    assert_eq!(
        m.signatures.len(),
        2,
        "pub + pub(crate) fns are the fan-out surface"
    );
    // private method inside impl is inventory knowledge
    assert!(m
        .inventory
        .iter()
        .any(|i| i.kind == "fn" && i.name == "default"));
}

#[test]
fn async_fns_are_inventoried_as_their_own_kind() {
    let root = std::env::temp_dir().join(format!("harna-async-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/m.rs"),
        "pub async fn handler() -> String { String::new() }\n\
         async fn private_poll() {}\n\
         fn sync_boring() {}\n\
         let boxed = async move { 1 }; // impossible in inventory tests\n\
         pub fn also_sync() {}\n\
         ",
    )
    .unwrap();
    let d = build(&root, 10).unwrap();
    let m = d.modules.iter().find(|m| m.path == "src/m.rs").unwrap();
    let names: Vec<&str> = m
        .inventory
        .iter()
        .filter(|i| i.kind == "async-fn")
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(
        names.len(),
        2,
        "pub and priv async fns inventoried: {names:?}"
    );
    assert!(names.contains(&"handler") && names.contains(&"private_poll"));
    // Non-fn or non-async declarations keep their kinds (move blocks
    // never parse as declarations at all).
    assert!(m
        .inventory
        .iter()
        .any(|i| i.kind == "fn" && i.name == "sync_boring"));
    assert!(!m.inventory.iter().any(|i| i.name == "boxed"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn whole_test_files_carry_no_deps_or_signatures() {
    for p in [
        "src/a/tests.rs",
        "src/a_tests.rs",
        "src/b_test.rs",
        "crates/x/tests/it.rs",
        "tests/common/mod.rs",
    ] {
        assert!(is_test_path(p), "{p}");
    }
    for p in [
        "src/a.rs",
        "src/testsuite.rs",
        "src/contest.rs",
        "src/latest.rs",
    ] {
        assert!(!is_test_path(p), "{p}");
    }
    let root =
        std::env::temp_dir().join(format!("harna-testfile-{:?}", std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/a")).unwrap();
    // A sibling test file: real-looking imports and a string literal
    // holding a `use` line — none of it is shipped surface.
    std::fs::write(
        root.join("src/a/tests.rs"),
        "use crate::b::helper;\npub fn fixture() {}\nconst SRC: &str = \"use crate::c::ghost;\";\n",
    )
    .unwrap();
    let m = scan_rust_file(&root, &root.join("src/a/tests.rs"), &[]);
    assert!(m.deps.is_empty(), "{:?}", m.deps);
    assert!(m.signatures.is_empty(), "{:?}", m.signatures);
    assert!(!m.body_hash.is_empty(), "still hashed: scans reuse it");
    let _ = std::fs::remove_dir_all(&root);
}

/// ledger field test (0.3.49): editing a method of `pub trait
/// TransactionRepository` left parity silent while `cargo check` failed.
/// Trait methods are surface, whole even when rustfmt wraps them.
#[test]
fn public_trait_methods_are_signatures() {
    let src = "pub trait TransactionRepository: Send {\n    fn insert(&self, tx: &Tx) -> Result<()>;\n    fn find(\n        &self,\n        id: TxId,\n        at: Date,\n    ) -> Result<Option<Tx>>;\n    fn count(&self) -> usize {\n        0\n    }\n}\ntrait Private {\n    fn hidden(&self);\n}\npub fn free() {}\n";
    let root = std::env::temp_dir().join(format!("harna-trait-sigs-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/repo.rs"), src).unwrap();
    let m = super::extract::scan_rust_file(&root, &root.join("src/repo.rs"), &[]);
    let traits: Vec<&String> = m
        .signatures
        .iter()
        .filter(|s| s.starts_with("trait "))
        .collect();
    assert_eq!(
        traits,
        vec![
            "trait TransactionRepository: fn insert(&self, tx: &Tx) -> Result<()>",
            "trait TransactionRepository: fn find(&self, id: TxId, at: Date) -> Result<Option<Tx>>",
            "trait TransactionRepository: fn count(&self) -> usize",
        ],
        "{:?}",
        m.signatures
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 0.7.7 (codex field test): `use other_crate::item` reaches that crate's
/// `lib.rs`, and through its `pub use sub::item` the module defining it;
/// an import of another name does not pass through.
#[test]
fn crate_imports_reach_the_root_and_its_reexports() {
    let module = |path: &str, deps: &[&str]| Module {
        path: path.into(),
        deps: deps.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    let lib = module("crates/fs/src/lib.rs", &["self::paths::canonicalize"]);
    let paths = module("crates/fs/src/paths.rs", &[]);
    let user = module("crates/app/src/main.rs", &["fs_util::canonicalize"]);
    let other = module("crates/app/src/other.rs", &["fs_util::Mode"]);
    let names = vec!["fs_util".to_string(), "app".to_string()];
    let dirs: std::collections::BTreeMap<String, String> =
        [("fs_util".to_string(), "crates/fs".to_string())].into();
    assert!(dep_reaches(&user, &user.deps[0], &lib, &names, &dirs));
    assert!(!dep_reaches(&user, &user.deps[0], &paths, &names, &dirs));
    assert!(reaches_through_root(
        &user,
        &user.deps[0],
        &lib,
        &paths,
        &names,
        &dirs
    ));
    assert!(!reaches_through_root(
        &other,
        &other.deps[0],
        &lib,
        &paths,
        &names,
        &dirs
    ));
    let all = vec![lib.clone(), paths.clone()];
    assert_eq!(
        crate_root_of(&all, &paths).map(|r| r.path.as_str()),
        Some("crates/fs/src/lib.rs")
    );
}
