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
    manifest: &crate::schema::model::manifest::Manifest,
    out: &mut Vec<Violation>,
) {
    // Cheap gate first: only missing paths pay for the worktree walk.
    // Children count too: a compound's child file moves like any other.
    let mut missing: Vec<(&str, &str)> = Vec::new();
    fn collect<'m>(
        root: &Path,
        comps: &'m [crate::schema::model::manifest::Component],
        out: &mut Vec<(&'m str, &'m str)>,
    ) {
        for c in comps {
            if !c.path.is_empty() && !root.join(&c.path).exists() {
                out.push((c.name.as_str(), c.path.as_str()));
            }
            collect(root, &c.children, out);
        }
    }
    for c in manifest.layers.iter().flat_map(|l| &l.containers) {
        collect(root, &c.components, &mut missing);
    }
    if missing.is_empty() {
        return;
    }

    let live = crate::search::digest::respected_rust_files(root, 10_000);
    for (name, path) in missing {
        let Some(want) = identity(path) else { continue };
        let mut hits: Vec<&str> = live
            .iter()
            .map(|s| s.as_str())
            .filter(|s| *s != path && identity(s).as_deref() == Some(want.as_str()))
            .collect();
        hits.sort_unstable();
        if hits.is_empty() {
            continue; // manifest-path owns the plain-missing case
        }
        // One file, one finding: a move candidate is the more useful
        // reading of "missing", so it replaces manifest-path's (both are
        // Errors — the gate is unchanged, only the nag is single).
        out.retain(|v| !(v.check == "manifest-path" && v.file == path));
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

/// What a file is recognisable by after a move. A distinctive name is
/// enough (`moved.rs`); a conventional one (`mod.rs`, `lib.rs`, …) names
/// nothing on its own, so its parent folder rides along (`billing/mod.rs`)
/// — otherwise every `mod.rs` in the repo would be a "candidate".
fn identity(path: &str) -> Option<String> {
    let p = Path::new(path);
    let name = p.file_name()?.to_str()?;
    if matches!(name, "mod.rs" | "lib.rs" | "main.rs" | "build.rs") {
        let parent = p.parent()?.file_name()?.to_str()?;
        Some(format!("{parent}/{name}"))
    } else {
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with(paths: &[&str]) -> crate::schema::model::manifest::Manifest {
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
        assert!(
            v[0].message.contains("src/sub/moved.rs"),
            "{}",
            v[0].message
        );
        assert!(v[0].hint.contains("update the manifest"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn conventional_names_match_with_their_folder_and_replace_manifest_path() {
        let dir = std::env::temp_dir().join(format!("harna-rename-mod-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["src/new_home/billing", "src/a", "src/b"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in [
            "src/new_home/billing/mod.rs",
            "src/a/mod.rs",
            "src/b/mod.rs",
        ] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let m = manifest_with(&["src/billing/mod.rs"]);
        let mut v = Vec::new();
        super::super::manifest::check_paths_exist(&dir, &m, &mut v);
        assert!(v
            .iter()
            .any(|v| v.check == "manifest-path" && v.file == "src/billing/mod.rs"));
        check(&dir, &m, &mut v);
        let renamed: Vec<_> = v.iter().filter(|v| v.check == "rename-linkage").collect();
        assert_eq!(renamed.len(), 1, "{v:#?}");
        assert!(renamed[0].message.contains("src/new_home/billing/mod.rs"));
        assert!(
            !renamed[0].message.contains("src/a/mod.rs"),
            "unrelated mod.rs is no candidate"
        );
        assert!(
            !v.iter()
                .any(|v| v.check == "manifest-path" && v.file == "src/billing/mod.rs"),
            "one file, one finding: {v:#?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn moved_children_are_linked_too() {
        let dir = std::env::temp_dir().join(format!("harna-rename-child-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/elsewhere")).unwrap();
        std::fs::write(dir.join("src/elsewhere/child_core.rs"), "").unwrap();
        let m: crate::schema::model::manifest::Manifest = toml::from_str(
            "schema_version = 1\n[[layers]]\nname = \"l\"\n[[layers.containers]]\nname = \"c\"\npath = \"src\"\n\
             [[layers.containers.components]]\nname = \"p\"\nkind = \"compound\"\npath = \"src/p/mod.rs\"\nsummary = \"s\"\n\
             [[layers.containers.components.children]]\nname = \"child\"\nkind = \"primitive\"\npath = \"src/p/child_core.rs\"\nsummary = \"s\"\n",
        )
        .unwrap();
        let mut v = Vec::new();
        check(&dir, &m, &mut v);
        assert!(
            v.iter().any(|v| v.check == "rename-linkage"
                && v.message.contains("src/elsewhere/child_core.rs")),
            "{v:#?}"
        );
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
