//! `ignore-reason` (E4): every ignored test says why. deno's node_compat
//! list carries 386 `"ignore": true` entries, each with a `reason`; an
//! ignored test without one is a hole nobody can review or re-arm. Rust's
//! `#[ignore = "…"]` holds the reason in the attribute. Unbound, the check
//! advises (registry `adopted_by_binding`); bound, a bare `#[ignore]`
//! gates — and a baseline can ratchet the count down.

use std::path::Path;

use super::{finding, Violation};

pub fn check(root: &Path, out: &mut Vec<Violation>) {
    for file in crate::search::digest::respected_rust_files(root, 50_000) {
        let Ok(src) = std::fs::read_to_string(root.join(&file)) else {
            continue;
        };
        // Test code is exactly what this reads — no live-code mask.
        for (i, line) in src.lines().enumerate() {
            let t = line.trim();
            if t == "#[ignore]" || t.starts_with("#[ignore]") {
                out.push(finding(
                    "ignore-reason",
                    &file,
                    i + 1,
                    "an ignored test without a reason".into(),
                    "write `#[ignore = \"why, and what re-arms it\"]`".into(),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_ignores_are_named() {
        let root = std::env::temp_dir().join(format!(
            "harna-ignores-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/a.rs"),
            "#[test]\n#[ignore]\nfn flaky() {}\n#[test]\n#[ignore = \"needs network; re-arm in CI\"]\nfn net() {}\n",
        )
        .unwrap();
        let mut out = Vec::new();
        check(&root, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!((out[0].file.as_str(), out[0].line), ("src/a.rs", 2));
        let _ = std::fs::remove_dir_all(&root);
    }
}
