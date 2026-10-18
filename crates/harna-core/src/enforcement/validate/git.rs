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
/// `core.quotePath=false` keeps non-ASCII paths verbatim; paths git still
/// quotes (spaces are not, but `"`, `\` and control chars are) are decoded
/// by `decode_path`.
fn porcelain(root: &Path, purpose: &str) -> anyhow::Result<String> {
    git(root, &["-c", "core.quotePath=false", "status", "--porcelain", "--untracked-files=all"], purpose)
}

/// The harna root's offset inside the git work tree (`""` at the top,
/// `"sub/dir/"` in a monorepo subdirectory). Porcelain paths are relative
/// to the work-tree top, while every consumer joins them onto the harna
/// root — so they are re-rooted through this prefix.
fn root_prefix(root: &Path, purpose: &str) -> anyhow::Result<String> {
    Ok(git(root, &["rev-parse", "--show-prefix"], purpose)?.trim().to_string())
}

fn git(root: &Path, args: &[&str], purpose: &str) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| anyhow::anyhow!("running git failed: {e} — {purpose}"))?;
    if !output.status.success() {
        anyhow::bail!("git {} failed — {purpose}", args.iter().find(|a| !a.starts_with('-') && !a.contains('=')).unwrap_or(&""));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Keep the paths under the harna root, relative to it; paths outside
/// (sibling packages of a monorepo) are not this root's changes.
pub(crate) fn reroot(paths: Vec<String>, prefix: &str) -> Vec<String> {
    paths
        .into_iter()
        .filter_map(|p| p.strip_prefix(prefix).map(str::to_string))
        .collect()
}

/// The path field of one porcelain line: the new side of a rename
/// (`old -> new`), each side possibly C-quoted (`"a\"b.rs"`, `"\303\244.rs"`).
fn line_path(field: &str) -> String {
    let field = field.trim_end();
    let new_side = if field.starts_with('"') {
        // A quoted old side may itself contain ` -> `: skip past its close.
        let close = closing_quote(field).unwrap_or(field.len() - 1);
        match field[close + 1..].strip_prefix(" -> ") {
            Some(rest) => rest,
            None => field,
        }
    } else {
        field.split_once(" -> ").map(|(_, new)| new).unwrap_or(field)
    };
    decode_path(new_side).replace('\\', "/")
}

fn closing_quote(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

fn is_oct(c: u8) -> bool {
    (b'0'..=b'7').contains(&c)
}

/// Undo git's C-style path quoting; unquoted paths pass through.
pub(crate) fn decode_path(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) else {
        return s.to_string();
    };
    let b = inner.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let c = b[i + 1];
        match c {
            b'0'..=b'7' if i + 3 < b.len() && is_oct(b[i + 2]) && is_oct(b[i + 3]) => {
                let oct = std::str::from_utf8(&b[i + 1..i + 4]).unwrap_or("0");
                out.push(u8::from_str_radix(oct, 8).unwrap_or(b'?'));
                i += 4;
                continue;
            }
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'v' => out.push(0x0b),
            other => out.push(other), // `\"`, `\\`
        }
        i += 2;
    }
    String::from_utf8_lossy(&out).into_owned()
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
            let path = line_path(&line[3..]);
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
    let purpose = "`--changed` requires a git repository";
    Ok(reroot(parse(&porcelain(root, purpose)?), &root_prefix(root, purpose)?))
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
            let path = line_path(&line[3..]);
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
    let purpose = "the born-size check needs a git repository";
    Ok(reroot(parse_new(&porcelain(root, purpose)?), &root_prefix(root, purpose)?))
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

    #[test]
    fn quoted_paths_are_decoded_not_dropped() {
        let porcelain = "\
?? \"src/q\\\"uote.rs\"
 M \"src/\\303\\244.rs\"
R  \"src/old -> name.rs\" -> \"src/new\\tname.rs\"
 M src/with space.rs
";
        let files = parse(porcelain);
        assert_eq!(
            files,
            vec![
                "src/q\"uote.rs".to_string(),
                "src/ä.rs".to_string(),
                "src/new\tname.rs".to_string(),
                "src/with space.rs".to_string(),
            ]
        );
        assert_eq!(parse_new("?? \"src/\\303\\244.rs\"\n"), vec!["src/ä.rs".to_string()]);
    }

    #[test]
    fn reroot_keeps_only_paths_under_the_harna_root() {
        let paths = vec![
            "pkg/a/src/lib.rs".to_string(),
            "pkg/b/src/lib.rs".to_string(),
            "top.rs".to_string(),
        ];
        assert_eq!(reroot(paths.clone(), ""), paths, "top-level root keeps all");
        assert_eq!(reroot(paths, "pkg/a/"), vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn changed_files_are_relative_to_a_subdirectory_root() {
        let repo = std::env::temp_dir()
            .join(format!("harna-git-subdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        let sub = repo.join("pkg/a");
        std::fs::create_dir_all(sub.join("src")).unwrap();
        std::fs::create_dir_all(repo.join("pkg/b")).unwrap();
        let git_ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !git_ok {
            return; // no git on this machine: the parser tests above cover the logic
        }
        std::fs::write(sub.join("src/my file.rs"), "").unwrap();
        std::fs::write(repo.join("pkg/b/other.rs"), "").unwrap();
        let files = changed_rust_files(&sub).unwrap();
        assert_eq!(files, vec!["src/my file.rs".to_string()]);
        assert!(sub.join(&files[0]).exists(), "joinable onto the harna root");
        let _ = std::fs::remove_dir_all(&repo);
    }
}
