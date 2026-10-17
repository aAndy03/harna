//! Component `rename-linkage` (0.3.31): the manifest's paths track
//! moves, not just existence.
//!
//! `manifest-path` already errors when a declared component path does
//! not exist. This check answers the next question: did the file MOVE?
//! For every declared component path missing on disk, the worktree is
//! searched for the same file name elsewhere — a hit means the linkage
//! is stale (update the manifest; the move may be deliberate, so harna
//! proposes, never rewrites). No candidate means silence here: the
//! plain-missing case belongs to `manifest-path`, and two checks must
//! never nag the same file twice.

use std::path::Path;

use super::{Severity, Violation};

pub fn check(
    root: &Path,
    manifest: &crate::schema::manifest::Manifest,
    out: &mut Vec<Violation>,
) {
    // Cheap gate first: only missing paths pay for the worktree walk.
    let missing: Vec<(&str, &str)> = manifest
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .flat_map(|c| &c.components)
        .filter(|c| !c.path.is_empty() && !root.join(&c.path).exists())
        .map(|c| (c.name.as_str(), c.path.as_str()))
        .collect();
    if missing.is_empty() {
        return;
    }

    let live = crate::search::digest::respected_rust_files(root, 10_000);
    for (name, path) in missing {
        let file_name = Path::new(path).file_name().and_then(|n| n.to_str());
        let Some(want) = file_name else { continue };
        let mut hits: Vec<&str> = live
            .iter()
            .map(|s| s.as_str())
            .filter(|s| {
                *s != path
                    && Path::new(s).file_name().and_then(|n| n.to_str()) == Some(want)
            })
            .collect();
        hits.sort_unstable();
        if hits.is_empty() {
            continue; // manifest-path owns the plain-missing case
        }
        out.push(Violation {
            check: "rename-linkage".into(),
            file: path.into(),
            line: 0,
            message: format!(
                "component `{name}` declares `{path}` (missing) — same file name lives at {}",
                hits.join(", ")
            ),
            hint: "if this was a move, update the manifest path (and any rule scopes naming the old one); if the file is gone, manifest-path already says so"
                .into(),
            severity: Severity::Error,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with(paths: &[&str]) -> crate::schema::manifest::Manifest {
        let toml_src = format!(
            "schema_version = 1\n\n[[layers]]\nname = \"memory\"\n\n[[layers.containers]]\nname = \"schema\"\npath = \"src\"\ndescription = \"d\"\n{}",
            paths
                .iter()
                .map(|p| format!(
                    "\n[[layers.containers.components]]\nname = \"c\"\nkind = \"primitive\"\npath = \"{p}\"\nsummary = \"s\"\n"
                ))
                .collect::<String>()
        );
        toml::from_str(&toml_src).unwrap()
    }

    #[test]
    fn a_moved_file_is_linked_not_just_missing() {
        let dir = std::env::temp_dir().join(format!("harna-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/sub")).unwrap();
        std::fs::write(dir.join("src/sub/moved.rs"), "pub fn f() {}\n").unwrap();

        let m = manifest_with(&["src/moved.rs"]);
        let mut v = Vec::new();
        check(&dir, &m, &mut v);
        assert_eq!(v.len(), 1, "{v:#?}");
        assert_eq!(v[0].check, "rename-linkage");
        assert!(v[0].message.contains("src/sub/moved.rs"), "{}", v[0].message);
        assert!(v[0].hint.contains("update the manifest"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plain_missing_stays_silent_here() {
        let dir = std::env::temp_dir().join(format!("harna-rename-gone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let m = manifest_with(&["src/gone.rs"]);
        let mut v = Vec::new();
        check(&dir, &m, &mut v);
        assert!(v.is_empty(), "no candidate, no nag: manifest-path owns it");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
