use super::*;

fn repo(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("harna-linkage-{tag}-{:?}", std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("crates/x/src")).unwrap();
    std::fs::write(d.join("crates/x/src/lib.rs"), "//! The x crate.\n\npub mod existing;\n\n#[cfg(test)]\nmod tests;\n").unwrap();
    d
}

fn read(root: &Path, p: &str) -> String {
    std::fs::read_to_string(root.join(p)).unwrap()
}

#[test]
fn a_new_file_is_wired_through_every_level_to_the_crate_root() {
    let root = repo("chain");
    std::fs::create_dir_all(root.join("crates/x/src/engine/billing")).unwrap();
    std::fs::write(root.join("crates/x/src/engine/billing/ledger.rs"), "").unwrap();
    let w = wire_module(&root, "crates/x/src/engine/billing/ledger.rs").unwrap();
    assert!(w.skipped.is_none(), "{w:?}");
    assert_eq!(
        w.created,
        vec!["crates/x/src/engine/billing/mod.rs".to_string(), "crates/x/src/engine/mod.rs".to_string()]
    );
    assert!(read(&root, "crates/x/src/engine/billing/mod.rs").contains("pub mod ledger;"));
    assert!(read(&root, "crates/x/src/engine/mod.rs").contains("pub mod billing;"));
    let lib = read(&root, "crates/x/src/lib.rs");
    // Beside its sibling, never after the test module.
    assert!(lib.contains("pub mod existing;\npub mod engine;\n"), "{lib}");
    assert!(lib.ends_with("#[cfg(test)]\nmod tests;\n"));
    // Idempotent: a second wiring changes nothing.
    let again = wire_module(&root, "crates/x/src/engine/billing/ledger.rs").unwrap();
    assert!(again.declared.is_empty() && again.created.is_empty(), "{again:?}");
    assert_eq!(again.render(), "already wired into the module tree");
}

#[test]
fn an_existing_parent_file_style_is_respected() {
    let root = repo("style");
    // `engine.rs` (2018 style) owns `engine/`: no mod.rs is created.
    std::fs::write(root.join("crates/x/src/engine.rs"), "//! Engine.\n").unwrap();
    std::fs::create_dir_all(root.join("crates/x/src/engine")).unwrap();
    std::fs::write(root.join("crates/x/src/engine/pump.rs"), "").unwrap();
    let w = wire_module(&root, "crates/x/src/engine/pump.rs").unwrap();
    assert!(w.created.is_empty(), "{w:?}");
    assert_eq!(read(&root, "crates/x/src/engine.rs"), "//! Engine.\n\npub mod pump;\n");
    // A compound's mod.rs wires its folder name.
    std::fs::create_dir_all(root.join("crates/x/src/hub")).unwrap();
    std::fs::write(root.join("crates/x/src/hub/mod.rs"), "").unwrap();
    wire_module(&root, "crates/x/src/hub/mod.rs").unwrap();
    assert!(read(&root, "crates/x/src/lib.rs").contains("pub mod hub;"));
}

#[test]
fn wiring_refuses_what_cannot_compile_and_writes_nothing() {
    let root = repo("refuse");
    std::fs::write(root.join("crates/x/src/user-repo.rs"), "").unwrap();
    let before = read(&root, "crates/x/src/lib.rs");
    let w = wire_module(&root, "crates/x/src/user-repo.rs").unwrap();
    assert!(w.render().contains("not a Rust module name"), "{w:?}");
    assert_eq!(read(&root, "crates/x/src/lib.rs"), before);
    // No crate root: nothing is created anywhere.
    std::fs::create_dir_all(root.join("loose/dir")).unwrap();
    std::fs::write(root.join("loose/dir/a.rs"), "").unwrap();
    let w = wire_module(&root, "loose/dir/a.rs").unwrap();
    assert!(w.render().contains("no crate root"));
    assert!(!root.join("loose/dir/mod.rs").exists());
    assert!(is_module_name("ledger_v2") && !is_module_name("Ledger") && !is_module_name("2fast"));
    assert_eq!(module_name("user-repo"), "user_repo");
}

#[test]
fn empty_tests_fields_get_the_tests_that_exist() {
    let root = repo("tests");
    let src = root.join("crates/x/src");
    std::fs::write(src.join("inline.rs"), "pub fn f() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n").unwrap();
    std::fs::write(src.join("split.rs"), "pub fn f() {}\n#[cfg(test)]\nmod tests;\n").unwrap();
    std::fs::create_dir_all(src.join("split")).unwrap();
    std::fs::write(src.join("split/tests.rs"), "#[test]\nfn t() {}\n").unwrap();
    std::fs::write(src.join("bare.rs"), "pub fn f() {}\n").unwrap();
    std::fs::write(src.join("kept.rs"), "#[test]\nfn t() {}\n").unwrap();
    let mut m: Manifest = toml::from_str(
        "schema_version = 1\n[[layers]]\nname = \"l\"\n[[layers.containers]]\nname = \"c\"\npath = \"crates/x/src\"\n\
         [[layers.containers.components]]\nname = \"inline\"\nkind = \"primitive\"\npath = \"crates/x/src/inline.rs\"\nsummary = \"s\"\n\
         [[layers.containers.components]]\nname = \"split\"\nkind = \"primitive\"\npath = \"crates/x/src/split.rs\"\nsummary = \"s\"\n\
         [[layers.containers.components]]\nname = \"bare\"\nkind = \"primitive\"\npath = \"crates/x/src/bare.rs\"\nsummary = \"s\"\n\
         [[layers.containers.components]]\nname = \"kept\"\nkind = \"primitive\"\npath = \"crates/x/src/kept.rs\"\nsummary = \"s\"\ntests = [\"elsewhere.rs\"]\n",
    )
    .unwrap();
    let registered = register_tests(&root, &mut m);
    assert_eq!(registered, vec!["inline".to_string(), "split".to_string()]);
    let comps = &m.layers[0].containers[0].components;
    assert_eq!(comps[0].tests, vec!["crates/x/src/inline.rs".to_string()]);
    assert_eq!(comps[1].tests, vec!["crates/x/src/split/tests.rs".to_string()]);
    assert!(comps[2].tests.is_empty(), "no tests, nothing invented");
    assert_eq!(comps[3].tests, vec!["elsewhere.rs".to_string()], "a declared field is never overwritten");
    assert!(register_tests(&root, &mut m).is_empty(), "idempotent");
}
