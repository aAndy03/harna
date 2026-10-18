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
