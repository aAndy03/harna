//! Component `git`: change selection for diff-scoped validation.
//!
//! `changed_rust_files` parses `git status --porcelain`; the enforcement
//! loop (`--changed`) scopes to exactly the files with uncommitted changes.
//! `new_rust_files` (0.3.26) keeps only the files BORN in the working
//! tree — the input for the `born-size` birth budget.

use std::path::Path;

/// One `git status` read shared by change selection and birth detection.
/// `--untracked-files=all` expands untracked DIRECTORIES into their files:
/// without it a brand-new `src/` shows as one `?? src/` line and every
/// new file inside goes blind (the 0.3.32 dogfood finding — staged adds
/// were always seen, unstaged births were not).
fn porcelain(root: &Path, purpose: &str) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output()
        .map_err(|e| anyhow::anyhow!("running git failed: {e} — {purpose}"))?;
    if !output.status.success() {
        anyhow::bail!("git status failed — {purpose}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}


/// Parse `git status --porcelain` output into relative `.rs` file paths.
/// Handles staged (`M  `), unstaged (` M `), untracked (`?? `) and rename
/// (`R  old -> new`) entries; keeps the new side of renames.
pub fn parse(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let mut path = line[3..].to_string();
            if let Some((_, new)) = path.split_once(" -> ") {
                path = new.to_string();
            }
            let path = path.trim().replace('\\', "/");
            if path.ends_with(".rs") {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}

/// `.rs` files with uncommitted changes — the input for `validate --changed`.
pub fn changed_rust_files(root: &Path) -> anyhow::Result<Vec<String>> {
    Ok(parse(&porcelain(root, "`--changed` requires a git repository")?))
}

/// Parse `git status --porcelain`; keep only files BORN in the working
/// tree: staged adds (`A `), adds later modified again (`AM`), and
/// untracked entries (`??`). A rename has hash history — the old file
/// exists — so its new side is a change, not a birth. Unparseable
/// indices can only be added; `!!`-ignored paths never reach status.
pub(crate) fn parse_new(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let code = &line[..2];
            if !code.starts_with('A') && code != "??" {
                return None;
            }
            let mut path = line[3..].to_string();
            if let Some((_, new)) = path.split_once(" -> ") {
                path = new.to_string();
            }
            let path = path.trim().replace('\\', "/");
            if path.ends_with(".rs") {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}

/// `.rs` files BORN in the working tree (added / untracked) — the input
/// for the `born-size` birth budget. Errors only when git is missing or
/// broken; callers in non-git repos skip the check.
pub fn new_rust_files(root: &Path) -> anyhow::Result<Vec<String>> {
    Ok(parse_new(&porcelain(
        root,
        "the born-size check needs a git repository",
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_status_parsing_handles_all_states() {
        let porcelain = "\
M  src/api/handlers.rs
 M src/engine/scanner.rs
?? src/new_module.rs
R  src/old.rs -> src/renamed.rs
 M README.md
A  src/added.rs
MM src/modified_twice.rs
";
        let files = parse(porcelain);
        assert!(files.contains(&"src/api/handlers.rs".to_string()));
        assert!(files.contains(&"src/engine/scanner.rs".to_string()));
        assert!(files.contains(&"src/new_module.rs".to_string()));
        // renames keep the new path
        assert!(files.contains(&"src/renamed.rs".to_string()));
        assert!(!files.contains(&"src/old.rs".to_string()));
        // non-rust files are filtered out
        assert!(!files.contains(&"README.md".to_string()));
        assert_eq!(files.len(), 6);
    }

    #[test]
    fn collapsed_untracked_dirs_stay_invisible_to_the_parser() {
        // Without --untracked-files=all git collapses a new tree to one
        // `?? src/` line — the parser drops it (not a file), which is why
        // the flag matters, not the parser. This pins the contract.
        let files = parse("?? src/\n");
        assert!(files.is_empty(), "a directory is not a file: {files:?}");
    }

    #[test]
    fn empty_status_gives_no_files() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn only_births_survive_the_new_file_filter() {
        let porcelain = "\
A  src/born_staged.rs
AM src/born_then_edited.rs
?? src/born_untracked.rs
M  src/edited.rs
 M src/edited_later.rs
R  src/old.rs -> src/renamed.rs
MM src/edited_twice.rs
?? README.md
A  docs/note.md
";
        let files = parse_new(porcelain);
        assert!(files.contains(&"src/born_staged.rs".to_string()));
        assert!(files.contains(&"src/born_then_edited.rs".to_string()));
        assert!(files.contains(&"src/born_untracked.rs".to_string()));
        // History-carrying states are changes, not births.
        assert!(!files.contains(&"src/edited.rs".to_string()));
        assert!(!files.contains(&"src/edited_later.rs".to_string()));
        assert!(!files.contains(&"src/renamed.rs".to_string()));
        assert!(!files.contains(&"src/old.rs".to_string()));
        // Non-rust files are filtered out.
        assert!(!files.contains(&"README.md".to_string()));
        assert!(!files.contains(&"docs/note.md".to_string()));
        assert_eq!(files.len(), 3);
    }
}
