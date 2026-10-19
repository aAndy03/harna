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
