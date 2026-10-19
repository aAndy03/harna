use super::*;

fn lines(src: &str) -> Vec<usize> {
    live_code(src).into_iter().map(|(n, _)| n).collect()
}

#[test]
fn a_gated_use_at_the_top_does_not_hide_the_file() {
    // font-manager probe: `#[cfg(test)] use …;` then a real `std::fs::read`
    let src = "#[cfg(test)]\nuse std::path::PathBuf;\n\npub fn probe(p: &Path) { let _ = std::fs::read(p); }\n";
    assert_eq!(lines(src), vec![3, 4]);
}

#[test]
fn gated_mod_declaration_and_inline_attribute_forms() {
    let src = "#[cfg(test)] mod fixtures;\npub mod parser;\n#[cfg(test)]\npub mod helpers;\nfn live() {}\n";
    assert_eq!(lines(src), vec![2, 5]);
}

#[test]
fn test_module_braces_are_skipped_then_code_resumes() {
    let src = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { let s = \"}\"; let c = '{'; }\n    #[test]\n    fn u() {}\n}\nfn b() {}\n";
    assert_eq!(lines(src), vec![1, 8]);
}

#[test]
fn test_fns_with_multiline_signatures_and_extra_attributes() {
    let src = "#[test]\n#[ignore]\nfn slow(\n    x: u8,\n) {\n    x.unwrap();\n}\nfn live<'a>(s: &'a str) {}\n";
    assert_eq!(lines(src), vec![8]);
}

#[test]
fn comments_are_masked_and_bare_mod_tests_is_an_item() {
    let src =
        "// fs::write in a comment\n/// doc .unwrap()\nmod tests {\n  fn t() {}\n}\nfn z() {}\n";
    assert_eq!(lines(src), vec![6]);
}

#[test]
fn deps_below_a_gated_use_reach_the_map() {
    // extractor v5: the layer-boundary check sees `crate::ui` again
    let src = "#[cfg(test)]\nuse crate::backend::metadata::fixtures;\nuse crate::ui::App;\n";
    let deps = crate::search::digest::extract_deps(src, &[], &[]);
    assert!(
        deps.iter().any(|d| d.starts_with("crate::ui")),
        "was: {deps:?}"
    );
    assert!(
        !deps.iter().any(|d| d.contains("fixtures")),
        "was: {deps:?}"
    );
}

#[test]
fn pub_use_lines_yield_their_public_names() {
    use crate::search::digest::reexported_names as r;
    assert_eq!(r("pub use deno_os::exit;"), vec!["exit"]);
    assert_eq!(r("pub use a::b::{C, d as E, self};"), vec!["C", "E"]);
    assert_eq!(r("pub use anyhow;"), vec!["anyhow"]);
    assert_eq!(r("pub use crate::ops::*;"), vec!["*"]);
    assert!(r("use std::fs;").is_empty());
    assert!(
        r("pub(crate) use x::Y;").is_empty(),
        "crate-private re-export"
    );
}

/// zed (0.3.46): the per-line needle rebuild made a 1.78M-line scan take
/// 15s. One pass over `::`, with word boundaries: `my_crate::x` is not
/// `crate::x`, `a::crate::x` is not a root path.
#[test]
fn inline_refs_find_known_heads_on_word_boundaries() {
    use crate::search::digest::inline_crate_refs as r;
    let names = vec!["harna-core".to_string(), "gpui".to_string()];
    assert_eq!(
        r(
            "let s = harna_core::schema::Store::open(gpui::App::new());",
            &names
        ),
        vec!["harna-core::schema::Store::open", "gpui::App::new"]
    );
    assert_eq!(r("x(crate::a::b)", &names), vec!["crate::a::b"]);
    assert!(r("my_crate::x(); other::gpui::y();", &names).is_empty());
}

/// zed (0.3.46): `use std::{\n path::PathBuf,\n}` read `path::PathBuf` as
/// an import of zed's own `path` crate; `use client::{.., proto};` recorded
/// nothing. Whole statements, expanded per item.
#[test]
fn use_statements_are_read_whole_and_per_item() {
    let names = vec!["path".to_string(), "client".to_string(), "fs".to_string()];
    let src = "use std::{\n    path::PathBuf,\n    fs,\n};\nuse client::{ChannelId, proto, user::{User, UserStore}};\nfn f() { path::local(); }\n";
    let deps = crate::search::digest::extract_deps(src, &names, &[]);
    assert!(deps.contains(&"client::ChannelId".to_string()), "{deps:?}");
    assert!(deps.contains(&"client::proto".to_string()), "{deps:?}");
    assert!(deps.contains(&"client::user".to_string()), "{deps:?}");
    assert!(
        deps.contains(&"path::local".to_string()),
        "a real inline use of the crate counts"
    );
    assert!(
        !deps.iter().any(|d| d.starts_with("path::PathBuf")),
        "{deps:?}"
    );
    assert!(!deps.iter().any(|d| d == "fs"), "{deps:?}");
}

/// ruff (0.3.46): `use crate::rules::ruff;` then `ruff::rules::X` is the
/// local module, not the workspace crate `ruff`; a declared `mod` shadows
/// too. Reading them as the crate invented impossible cross-crate cycles.
#[test]
fn local_names_shadow_crate_names() {
    let names = vec!["ruff".to_string(), "gpui".to_string()];
    let src = "use crate::rules::ruff;\nuse gpui::App as GApp;\nfn f() { ruff::rules::check(); gpui::run(); }\n";
    let deps = crate::search::digest::extract_deps(src, &names, &[]);
    assert!(!deps.iter().any(|d| d.starts_with("ruff::")), "{deps:?}");
    assert!(deps.contains(&"crate::rules::ruff".to_string()));
    assert!(deps.contains(&"gpui::run".to_string()), "{deps:?}");
    let shadowed = crate::search::digest::extract_deps(
        "fn g() { gpui::x(); }\n",
        &names,
        &["gpui".to_string()],
    );
    assert!(
        !shadowed.iter().any(|d| d.starts_with("gpui::")),
        "{shadowed:?}"
    );
}

/// zed (0.3.47, H9): `quote! { gpui::BenchReport::default() }` in the
/// gpui_macros proc-macro crate is code it emits into callers, not a
/// dependency of gpui_macros. Live code around the macro stays.
#[test]
fn emitted_code_in_macro_bodies_is_masked() {
    let src = "let r = match fps {\n    Some(f) => quote! { gpui::BenchReport::with_fps(#f) },\n    None => quote! {\n        gpui::BenchReport::default()\n    },\n};\nlet k = helpers::kind();\nmacro_rules! m {\n    ($x:expr) => { std::fs::read($x) };\n}\nfn after() {}\n";
    let live: Vec<&str> = live_code(src).into_iter().map(|(_, l)| l).collect();
    let all = live.join(" | ");
    assert!(!all.contains("gpui::"), "{all}");
    assert!(!all.contains("std::fs::read"), "{all}");
    assert!(all.contains("Some(f) =>"), "{all}");
    assert!(all.contains("helpers::kind()"), "{all}");
    assert!(all.contains("fn after()"), "{all}");
    let deps = crate::search::digest::extract_deps(src, &["gpui".to_string()], &[]);
    assert!(!deps.iter().any(|d| d.starts_with("gpui")), "{deps:?}");
}

/// zed (0.3.47): a path in a message — on one line or a `\`-continued one —
/// is text; `#[cfg(all(test, …))]` gates a test item.
#[test]
fn strings_are_text_and_all_test_cfgs_gate() {
    let mut st = StrState::default();
    assert_eq!(
        code_only(r##"x.error("#[gpui::bench] once"); y::z"##, &mut st),
        r#"x.error(""); y::z"#
    );
    assert_eq!(code_only(r#"anyhow!("not \"#, &mut st), r#"anyhow!(""#);
    assert_eq!(code_only("through gpui::bench_main! unless", &mut st), "");
    assert_eq!(code_only(r#"none")); a::b"#, &mut st), r#"")); a::b"#);
    assert_eq!(
        code_only(r##"r#"q::w"# + '"' + c::d"##, &mut st),
        r#""" +  + c::d"#
    );
    assert_eq!(
        code_only("fn f<'a>(x: &'a T) -> m::N", &mut st),
        "fn f<'a>(x: &'a T) -> m::N"
    );

    let src =
        "#[cfg(all(test, target_os = \"linux\"))]\nmod t {\n    use gpui::x;\n}\nfn live() {}\n";
    let lines: Vec<&str> = live_code(src).into_iter().map(|(_, l)| l).collect();
    assert_eq!(lines, vec!["fn live() {}"]);
}
