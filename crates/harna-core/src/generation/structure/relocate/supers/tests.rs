use super::*;

/// ledger field test (0.3.49): moving `application/report.rs` into
/// `application/reporting/` left its `super::ports` pointing at the new
/// parent. Top-level `super::` becomes the old parent's absolute path; an
/// inline test module's `super::*` is the moved module and stays.
#[test]
fn top_level_super_paths_follow_the_old_parent() {
    let src = "use super::ports::Repo;\nuse super::super::domain::Money;\n// super::x in a comment\n\
               pub fn f() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\nfn g() { super::helper(); }\n";
    let out = rewrite(src, "app::services::report").unwrap();
    assert_eq!(
        out,
        "use crate::app::services::ports::Repo;\nuse crate::app::domain::Money;\n// super::x in a comment\n\
         pub fn f() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\nfn g() { crate::app::services::helper(); }\n"
    );
    assert_eq!(
        rewrite("use super::x;\n", "report").as_deref(),
        Some("use crate::x;\n")
    );
    assert!(rewrite("fn a() {}\n", "a::b").is_none());
}
