//! Check `test-weakened` (1.1.3, info): a change that removes more
//! assertions than it adds, or adds a skip (`#[ignore]`, `.skip(`,
//! `.only(`, `xit(`), is named — the reason belongs on record
//! (`harna acknowledge`), not in the agent's head. Agents under pressure
//! "pass" a suite by weakening what it asserts.
//!
//! Text, not parse trees: assertion and skip markers across Rust and the
//! common JavaScript runners.

use std::path::Path;

use super::super::{Severity, Violation};

/// Lines that assert something, in Rust: the assert macros only (review
/// of 1.1.4: `Result::expect` turned into `?` read as a weakened test).
const RUST_ASSERTS: &str = r"\b(?:debug_)?assert(?:_eq|_ne|_matches)?!";
/// And in the JavaScript runners.
const ASSERTS: &str = r"\bexpect\(|\bassert\.|\.to(?:Be|Equal|StrictEqual|Match|Throw|Contain|HaveBeenCalled)\w*\(|\.should\.";
/// Lines that skip or focus a test.
const SKIPS: &str = r"#\[ignore\b|\b(?:it|test|describe)\.(?:skip|only|todo)\(|\bx(?:it|describe|test)\(|\bpending\(";

/// At most this many changed files are read (one `git diff` each).
const MAX_FILES: usize = 200;

pub fn check(
    root: &Path,
    base: &str,
    changed: &[super::super::git::Change],
    out: &mut Vec<Violation>,
) {
    let (Ok(rust), Ok(web), Ok(skips)) = (
        regex::Regex::new(RUST_ASSERTS),
        regex::Regex::new(ASSERTS),
        regex::Regex::new(SKIPS),
    ) else {
        return;
    };
    let sources = changed
        .iter()
        .filter(|c| {
            c.status != 'D'
                && (c.path.ends_with(".rs") || crate::search::digest::ts::is_source(&c.path))
        })
        .take(MAX_FILES);
    for c in sources {
        let path = &c.path;
        let asserts = if path.ends_with(".rs") { &rust } else { &web };
        let (removed, added) = super::super::git::line_changes(root, base, c);
        let count = |lines: &[String], re: &regex::Regex| {
            lines
                .iter()
                .filter(|l| !l.trim_start().starts_with("//") && re.is_match(l))
                .count()
        };
        let lost = count(&removed, asserts).saturating_sub(count(&added, asserts));
        let skipped = count(&added, &skips).saturating_sub(count(&removed, &skips));
        if lost == 0 && skipped == 0 {
            continue;
        }
        let mut what = Vec::new();
        if lost > 0 {
            what.push(format!("{lost} assertion(s) fewer"));
        }
        if skipped > 0 {
            what.push(format!("{skipped} test(s) newly skipped or focused"));
        }
        out.push(Violation {
            check: "test-weakened".into(),
            file: path.clone(),
            line: 0,
            severity: Severity::Info,
            message: format!(
                "tests weakened since {base}: {} — was the behaviour changed, or the test?",
                what.join(", ")
            ),
            hint: "if the behaviour changed on purpose, record why (`harna acknowledge add test-weakened --file <f> --reason ..`); if the test was in the way, restore it and fix the code".into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_markers_cover_rust_and_the_javascript_runners() {
        let rust = regex::Regex::new(RUST_ASSERTS).unwrap();
        assert!(rust.is_match("    assert_eq!(total, 3);"));
        assert!(!rust.is_match("let v = load().expect(\"config\");"));
        let asserts = regex::Regex::new(ASSERTS).unwrap();
        for l in [
            "expect(total).toBe(3);",
            "assert.equal(a, b);",
            "total.should.equal(3);",
        ] {
            assert!(asserts.is_match(l), "{l}");
        }
        let skips = regex::Regex::new(SKIPS).unwrap();
        for l in [
            "#[ignore]",
            "it.skip('x', () => {})",
            "describe.only('y', f)",
            "xit('z')",
        ] {
            assert!(skips.is_match(l), "{l}");
        }
        assert!(!skips.is_match("let skip = items.skip(3);"));
    }
}
