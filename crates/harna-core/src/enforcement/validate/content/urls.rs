//! Component `urls`: the hardcoded-URL check.
//!
//! Rule companion of `no-hardcoded-urls` (rules.toml). URLs belong in an
//! `Endpoint` constants module (or a data file — constants-are-data) so they
//! are swappable per environment.

use super::{Severity, Violation};

/// No `http://`/`https://` literals hardwired in source.
/// Carved out: endpoint/constants modules, and test sections (fixtures may
/// embed URLs; production code must not).
pub fn check(src: &str, file: &str, out: &mut Vec<Violation>) {
    let file_lower = file.to_lowercase();
    if file_lower.contains("endpoint") || file_lower.contains("constants") {
        return;
    }
    // Rust's `'` opens a char or a lifetime, not a string; the web
    // languages quote with all three (review of 1.1.2: `fetch('https://…')`
    // was missed).
    let quotes: &[char] = if file_lower.ends_with(".rs") {
        &['"']
    } else {
        &['"', '\'', '`']
    };
    for (n, line) in crate::search::digest::live::live_code(src) {
        if !in_string(line, quotes) {
            continue;
        }
        out.push(Violation {
            check: "no-hardcoded-urls".into(),
            file: file.into(),
            line: n,
            message: "hardcoded URL in source".into(),
            hint: "move the URL into the Endpoint constants module and reference it".into(),
            severity: Severity::Error,
        });
    }
}

/// A URL that sits inside a string literal on this line — an odd number of
/// one quote kind before it. XML namespaces (`xmlns`, the w3.org schema
/// names an inline SVG carries) are identifiers, not endpoints.
fn in_string(line: &str, quotes: &[char]) -> bool {
    // The schemes are spelled apart so this matcher is not its own hit.
    let mut from = 0;
    while let Some(at) = ["http", "https"]
        .iter()
        .filter_map(|s| line[from..].find(&format!("{s}:{}", "//")))
        .min()
    {
        let at = from + at;
        let before = &line[..at];
        let namespace = before.trim_end_matches(['"', '\'', '=']).ends_with("xmlns")
            || line[at..]
                .split_once("//")
                .is_some_and(|(_, host)| host.starts_with("www.w3.org/"));
        if !namespace && quotes.iter().any(|q| before.matches(*q).count() % 2 == 1) {
            return true;
        }
        from = at + 4;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardcoded_urls_flagged_but_not_in_constants_module() {
        let mut v = Vec::new();
        check(
            "fn main() {\n    fetch(\"https://api.example.com/users\");\n}",
            "src/api/users.rs",
            &mut v,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].check, "no-hardcoded-urls");
        assert_eq!(v[0].line, 2);

        let mut v2 = Vec::new();
        check(
            "pub const USERS: &str = \"https://api.example.com/users\";",
            "src/constants/endpoints.rs",
            &mut v2,
        );
        assert!(v2.is_empty());
    }

    #[test]
    fn comments_and_url_free_lines_are_ignored() {
        let mut v = Vec::new();
        check("// see https://example.com/docs", "src/x.rs", &mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn test_code_may_embed_urls() {
        let src = "pub fn run() {}\n\n#[cfg(test)]\nmod tests {\n    \
const FIXTURE: &str = \"https://test.example.com/\";\n}";
        let mut v = Vec::new();
        check(src, "src/run.rs", &mut v);
        assert!(v.is_empty());

        let src2 = "pub fn a() { fetch(\"https://prod.example.com\"); }\n\
#[cfg(test)]\nmod tests { /* fixture with https://fixture.example.com */ }";
        let mut v2 = Vec::new();
        check(src2, "src/a.rs", &mut v2);
        assert_eq!(v2.len(), 1);
        assert_eq!(v2[0].line, 1);
    }

    /// Review of 1.1.2 (M3): single quotes and template strings carry URLs
    /// in TypeScript; SVG namespaces are not endpoints.
    #[test]
    fn web_quotes_count_and_namespaces_do_not() {
        let mut v = Vec::new();
        check("fetch('https://api.example.com/x');", "src/api.ts", &mut v);
        check(
            "const u = `https://api.example.com/${id}`;",
            "src/b.ts",
            &mut v,
        );
        assert_eq!(v.len(), 2);
        let mut v = Vec::new();
        check(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink='http://www.w3.org/1999/xlink'>",
            "src/Icon.tsx",
            &mut v,
        );
        check(
            "fn f<'a>(x: &'a str) {} // https://doc.rust-lang.org",
            "src/a.rs",
            &mut v,
        );
        assert!(v.is_empty(), "{v:?}");
    }
}
