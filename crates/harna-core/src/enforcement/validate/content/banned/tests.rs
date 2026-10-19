use super::*;
use crate::schema::model::rules::Rule;

fn rules() -> Rules {
    let mut r = Rules::default();
    r.rules.push(Rule {
        id: "fs-through-the-trait".into(),
        text: "Filesystem access goes through the FileSystem trait".into(),
        rationale: "permissions are checked there".into(),
        scope: vec!["ext/fs".into()],
        origin: "test".into(),
        check: Some("banned-api".into()),
        values: vec!["std::fs::read".into(), "std::process::exit".into()],
        ..Default::default()
    });
    r
}

/// deno study B3 (0.3.45): `ext/fs/clippy.toml` bans `std::fs::read` —
/// "should be done using FileSystem trait". The same ban as rule data.
#[test]
fn banned_paths_fire_in_scope_including_module_aliases() {
    let src = "use std::fs;\nfn a() { let b = std::fs::read(p); }\nfn c() { fs::read(p); }\n\
               fn d() { fs::read_to_string(p); }\nfn e() { std::process::exit(1) }\n";
    let mut out = Vec::new();
    check(&rules(), None, "ext/fs/std_fs.rs", src, &mut out);
    let lines: Vec<usize> = out.iter().map(|v| v.line).collect();
    assert_eq!(
        lines,
        vec![2, 3, 5],
        "read_to_string is another path: {out:?}"
    );
    assert!(out[0].message.contains("`std::fs::read` is banned here"));
    assert!(out[0].hint.contains("permissions are checked there"));

    let mut elsewhere = Vec::new();
    check(&rules(), None, "runtime/x.rs", src, &mut elsewhere);
    assert!(elsewhere.is_empty(), "out of scope");
}

/// deno's ext/fs/std_fs.rs IS the FileSystem implementation: a reasoned
/// file-level `#![allow(clippy::disallowed_methods, reason = …)]` is the
/// same exemption clippy honors.
#[test]
fn the_reasoned_implementation_file_is_exempt() {
    let src = "#![allow(clippy::disallowed_methods, reason = \"file system implementation\")]\n\
               use std::fs;\nfn m() { fs::read(p); }\n";
    let mut out = Vec::new();
    check(&rules(), None, "ext/fs/std_fs.rs", src, &mut out);
    assert!(out.is_empty(), "{out:?}");
    let bare = src.replace(", reason = \"file system implementation\"", "");
    check(&rules(), None, "ext/fs/std_fs.rs", &bare, &mut out);
    assert_eq!(out.len(), 1, "an unreasoned allow exempts nothing");
}
