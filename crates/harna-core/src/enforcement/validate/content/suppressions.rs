//! `suppression-reason` (0.3.45, deno study B2): every lint suppression
//! says why. deno denies `allow_attributes_without_reason` and carries
//! 153 `allow(clippy::disallowed_*, reason = "…")` sites — each one an
//! exemption a reviewer can read. Unbound, the check advises (registry
//! `adopted_by_binding`); bound, a bare `#[allow(…)]` gates.

use super::Violation;
use crate::search::digest::live::live_code;

pub fn check(file: &str, src: &str, out: &mut Vec<Violation>) {
    let live = live_code(src);
    for (i, &(n, line)) in live.iter().enumerate() {
        let attr = ["#[allow(", "#![allow(", "#[expect(", "#![expect("]
            .iter()
            .find(|a| line.starts_with(**a));
        let Some(attr) = attr else { continue };
        // The whole attribute, however rustfmt wrapped it.
        let mut text = String::from(line);
        for (_, next) in live.iter().skip(i + 1) {
            if text.contains(")]") {
                break;
            }
            text.push_str(next);
        }
        if text.contains("reason") {
            continue;
        }
        out.push(Violation {
            check: "suppression-reason".into(),
            file: file.into(),
            line: n,
            severity: crate::enforcement::checks::severity_of("suppression-reason"),
            message: format!(
                "`{}…` suppresses a lint without saying why",
                attr.trim_end_matches('(')
            ),
            hint: "add `reason = \"…\"` (Rust 1.81+) — the exemption becomes reviewable".into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_suppressions_are_named_reasoned_ones_pass() {
        let src = "#![allow(clippy::disallowed_methods, reason = \"fs implementation\")]\n\
                   #[allow(dead_code)]\nfn a() {}\n#[expect(unused)]\nfn b() {}\n\
                   #[allow(\n    unused,\n    reason = \"wrapped by rustfmt\"\n)]\nfn c() {}\n\
                   // #[allow(x)] in a comment\n";
        let mut out = Vec::new();
        check("ext/fs/std_fs.rs", src, &mut out);
        let lines: Vec<usize> = out.iter().map(|v| v.line).collect();
        assert_eq!(lines, vec![2, 4], "{out:?}");
        assert!(out[0].message.contains("`#[allow…`"));
    }
}
