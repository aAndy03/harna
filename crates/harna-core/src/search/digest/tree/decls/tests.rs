use super::*;

/// zed's migrator: `mod m_2025_04_15 { mod keymap; }` declares
/// `migrations/m_2025_04_15/keymap.rs`.
#[test]
fn file_mods_nest_through_inline_modules() {
    let src = "pub(crate) mod a {\n    mod keymap;\n    pub mod settings;\n}\n\
               #[cfg(test)]\nmod tests;\n#[cfg(not(windows))] mod server;\n\
               mod inline { fn f() {} }\nmod after;\n\
               #[path = \"../wit/v8/settings.rs\"]\nmod wit;\n\
               mod r#enum;\n\
               #[cfg_attr(target_os = \"linux\", path = \"bubblewrap.rs\")]\n\
               #[cfg_attr(target_os = \"macos\", path = \"seatbelt.rs\")]\nmod backend;\n\
               mod sys { include!(\"iokit_bindings.rs\"); }\n\
               #[cfg_attr(\n    not(unix),\n    path = \"unavailable.rs\"\n)]\nmod devices;\n\
               include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n";
    assert_eq!(
        file_mods(src),
        vec![
            "a/keymap",
            "a/settings",
            "tests",
            "server",
            "after",
            "../wit/v8/settings.rs",
            "enum",
            "bubblewrap.rs",
            "seatbelt.rs",
            "iokit_bindings.rs",
            "unavailable.rs",
        ]
    );
}
