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
    let mut in_test_code = false;
    for (i, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        if !in_test_code
            && (trimmed.contains("#[cfg(test)]")
                || trimmed.starts_with("mod tests")
                || trimmed.starts_with("#[test]"))
        {
            in_test_code = true;
        }
        if in_test_code || trimmed.starts_with("//") {
            continue;
        }
        // Only count URLs inside string literals (heuristic: a quote on the line).
        if (line.contains("http://") || line.contains("https://")) && line.contains('"') {
            out.push(Violation {
                check: "no-hardcoded-urls".into(),
                file: file.into(),
                line: i + 1,
                message: "hardcoded URL in source".into(),
                hint: "move the URL into the Endpoint constants module and reference it".into(),
                severity: Severity::Error,
            });
        }
    }
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
}
