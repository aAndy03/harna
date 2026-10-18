use super::*;

fn ws(tag: &str) -> (std::path::PathBuf, Store) {
    let root = std::env::temp_dir().join(format!("harna-move-{tag}-{:?}", std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&root);
    let w = |p: &str, s: &str| {
        let f = root.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, s).unwrap();
    };
    w("Cargo.toml", "[workspace]\nmembers = [\"crates/core\", \"crates/cli\"]\n");
    w("crates/core/Cargo.toml", "[package]\nname = \"demo-core\"\nversion = \"0.1.0\"\n");
    w("crates/core/src/lib.rs", "pub mod schema;\n");
    w("crates/core/src/schema/mod.rs", "//! Schema.\n\npub mod terms_data;\npub mod store;\n");
    w("crates/core/src/schema/terms_data.rs", "pub struct Terms;\n#[cfg(test)]\nmod tests;\n");
    w("crates/core/src/schema/terms_data/tests.rs", "#[test]\nfn t() {}\n");
    w("crates/core/src/schema/store.rs", "use crate::schema::terms_data::Terms;\npub fn f(_: Terms) {}\nuse crate::schema::terms_data_extra as _x;\n");
    w("crates/cli/Cargo.toml", "[package]\nname = \"demo-cli\"\nversion = \"0.1.0\"\n");
    w("crates/cli/src/main.rs", "use demo_core::schema::terms_data::Terms;\nfn main() {}\n");
    let store = Store::open(&root).unwrap();
    store.init().unwrap();
    std::fs::write(
        store.manifest_path(),
        "schema_version = 1\n\n# Header kept.\n\n[[layers]]\nname = \"l\"\n[[layers.containers]]\nname = \"schema\"\npath = \"crates/core/src/schema\"\n\
         [[layers.containers.components]]\nname = \"terms\"\nkind = \"primitive\"\npath = \"crates/core/src/schema/terms_data.rs\"\nsummary = \"s\"\ntests = [\"crates/core/src/schema/terms_data/tests.rs\"]\n",
    )
    .unwrap();
    std::fs::write(
        store.rules_path(),
        "schema_version = 1\n[[rules]]\nid = \"r\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"o\"\nscope = [\"crates/core/src/schema/terms_data.rs\"]\n",
    )
    .unwrap();
    (root, store)
}

fn read(root: &Path, p: &str) -> String {
    std::fs::read_to_string(root.join(p)).unwrap()
}

#[test]
fn one_move_keeps_every_link() {
    let (root, store) = ws("links");
    let r = relocate(&root, &store, "crates/core/src/schema/terms_data.rs", "crates/core/src/schema/registers/terms.rs").unwrap();
    // Bytes, with the children folder.
    assert!(root.join("crates/core/src/schema/registers/terms.rs").is_file());
    assert!(root.join("crates/core/src/schema/registers/terms/tests.rs").is_file());
    assert!(!root.join("crates/core/src/schema/terms_data.rs").exists());
    // Manifest (path + tests) and rule scope, typed — header kept.
    let m = store.load_manifest().unwrap();
    let c = &m.layers[0].containers[0].components[0];
    assert_eq!(c.path, "crates/core/src/schema/registers/terms.rs");
    assert_eq!(c.tests, vec!["crates/core/src/schema/registers/terms/tests.rs".to_string()]);
    assert_eq!(r.manifest_paths, 2);
    assert!(read(&root, ".harna/manifest.toml").contains("# Header kept."));
    assert_eq!(store.load_rules().unwrap().rules[0].scope, vec!["crates/core/src/schema/registers/terms.rs".to_string()]);
    // Module tree: old declaration gone, new one wired (folder mod.rs born).
    assert_eq!(r.declaration_removed.as_deref(), Some("pub mod terms_data;"));
    assert!(!read(&root, "crates/core/src/schema/mod.rs").contains("terms_data"));
    assert!(read(&root, "crates/core/src/schema/mod.rs").contains("pub mod registers;"));
    assert!(read(&root, "crates/core/src/schema/registers/mod.rs").contains("pub mod terms;"));
    // Imports: same crate and sibling crate; a longer name is not touched.
    let store_rs = read(&root, "crates/core/src/schema/store.rs");
    assert!(store_rs.contains("use crate::schema::registers::terms::Terms;"), "{store_rs}");
    assert!(store_rs.contains("crate::schema::terms_data_extra"), "token boundary respected");
    assert!(read(&root, "crates/cli/src/main.rs").contains("use demo_core::schema::registers::terms::Terms;"));
    assert_eq!(r.module_path, Some(("schema::terms_data".into(), "schema::registers::terms".into())));
    let text = r.render();
    assert!(text.contains("rewritten in 2 file(s)"), "{text}");
}

#[test]
fn a_move_never_overwrites_or_escapes() {
    let (root, store) = ws("refuse");
    assert!(relocate(&root, &store, "crates/core/src/schema/terms_data.rs", "crates/core/src/schema/store.rs").is_err());
    assert!(relocate(&root, &store, "crates/core/src/schema/nope.rs", "crates/core/src/x.rs").is_err());
    assert!(relocate(&root, &store, "crates/core/src/schema/terms_data.rs", "../out.rs").is_err());
    assert!(root.join("crates/core/src/schema/terms_data.rs").is_file(), "nothing moved on refusal");
}

#[test]
fn path_replacement_respects_identifier_boundaries() {
    assert_eq!(replace_path("a::b::C a::bc x_a::b", "a::b", "z").as_deref(), Some("z::C a::bc x_a::b"));
    assert_eq!(replace_path("nothing here", "a::b", "z"), None);
}
