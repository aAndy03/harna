//! Component `crate-skeleton` (0.4.10, field test 049): a declared
//! container whose crate does not exist yet gets one — `Cargo.toml`, a
//! workspace member entry, and `src/lib.rs` or `src/main.rs` — so a
//! profile's shape becomes code without hand-written crate boilerplate.
//! Asked for (`--new-crate lib|bin`), never assumed.

use std::path::Path;

/// The crate directory a container path implies: everything before its
/// `src` segment (`crates/app/src/ui` -> `crates/app`).
pub(super) fn crate_dir_of(container_path: &str) -> Option<String> {
    let p = crate::schema::model::scope::normalize_scope(container_path);
    let segs: Vec<&str> = p.split('/').collect();
    let at = segs.iter().position(|s| *s == "src")?;
    if at == 0 {
        return Some(".".into());
    }
    Some(segs[..at].join("/"))
}

/// Create the crate at `dir`. Returns the files written (repo-relative).
pub(super) fn create(root: &Path, dir: &str, kind: &str) -> anyhow::Result<Vec<String>> {
    let entry = match kind {
        "lib" => "src/lib.rs",
        "bin" => "src/main.rs",
        other => anyhow::bail!("--new-crate takes lib or bin (got `{other}`)"),
    };
    if dir == "." {
        anyhow::bail!("--new-crate makes a member crate; the repo root is not one");
    }
    let base = root.join(dir);
    if base.join("Cargo.toml").exists() {
        anyhow::bail!("`{dir}/Cargo.toml` already exists — it is a crate; drop --new-crate");
    }
    let name = dir.rsplit('/').next().unwrap_or(dir).to_string();
    // no root manifest yet: the first crate starts the workspace (0.5.9)
    let mut written = Vec::new();
    if !root.join("Cargo.toml").exists() {
        crate::schema::store::atomic::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"2\"\n",
        )?;
        written.push("Cargo.toml (new workspace)".to_string());
    }
    let workspace = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let inherits = |key: &str| {
        toml::from_str::<toml::Value>(&workspace)
            .ok()
            .and_then(|v| v.get("workspace")?.get("package")?.get(key).cloned())
            .is_some()
    };
    let field = |key: &str, own: &str| {
        if inherits(key) {
            format!("{key}.workspace = true\n")
        } else {
            format!("{key} = \"{own}\"\n")
        }
    };
    let manifest = format!(
        "[package]\nname = \"{name}\"\n{}{}\n[dependencies]\n",
        field("version", "0.1.0"),
        field("edition", "2021"),
    );
    std::fs::create_dir_all(base.join("src"))?;
    crate::schema::store::atomic::write(base.join("Cargo.toml"), manifest)?;
    let body = if kind == "bin" {
        format!("//! `{name}` — the binary.\n\nfn main() {{}}\n")
    } else {
        format!("//! `{name}`.\n")
    };
    crate::schema::store::atomic::write(base.join(entry), body)?;
    written.extend([format!("{dir}/Cargo.toml"), format!("{dir}/{entry}")]);
    if let Some(updated) = with_member(&workspace, dir) {
        crate::schema::store::atomic::write(root.join("Cargo.toml"), updated)?;
        written.push("Cargo.toml (workspace member)".into());
    }
    Ok(written)
}

/// The workspace manifest with `dir` added to `members`, unless a member
/// pattern already covers it (or there is no `members` list to extend).
fn with_member(workspace: &str, dir: &str) -> Option<String> {
    let value: toml::Value = toml::from_str(workspace).ok()?;
    let members = value.get("workspace")?.get("members")?.as_array()?;
    let covered = members
        .iter()
        .filter_map(|m| m.as_str())
        .any(|m| crate::schema::model::scope::glob_match(m, dir));
    if covered {
        return None;
    }
    let re = regex::Regex::new(r"(?s)(members\s*=\s*\[)(.*?)(\])").ok()?;
    let caps = re.captures(workspace)?;
    let inner = caps.get(2)?.as_str();
    let trimmed = inner.trim_end();
    let sep = if trimmed.trim().is_empty() || trimmed.ends_with(',') {
        ""
    } else {
        ", "
    };
    let range = caps.get(2)?.range();
    let mut out = workspace.to_string();
    out.replace_range(range, &format!("{trimmed}{sep}\"{dir}\""));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0.4.10: the crate a profile declares becomes real in one step —
    /// inheriting the workspace's version/edition, listed as a member only
    /// when no pattern covers it already.
    #[test]
    fn a_declared_crate_is_born_and_joined() {
        let root = std::env::temp_dir().join(format!("harna-skeleton-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"tools/cli\"]\n[workspace.package]\nedition = \"2021\"\n",
        )
        .unwrap();
        assert_eq!(
            crate_dir_of("crates/app/src/ui").as_deref(),
            Some("crates/app")
        );
        assert_eq!(crate_dir_of("src/ui").as_deref(), Some("."));
        assert!(crate_dir_of("crates/app").is_none());
        let written = create(&root, "crates/app", "bin").unwrap();
        assert!(
            written.iter().any(|w| w.starts_with("Cargo.toml")),
            "{written:?}"
        );
        let cargo = std::fs::read_to_string(root.join("crates/app/Cargo.toml")).unwrap();
        assert!(
            cargo.contains("edition.workspace = true") && cargo.contains("version = \"0.1.0\""),
            "{cargo}"
        );
        assert!(root.join("crates/app/src/main.rs").exists());
        let ws = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(ws.contains("\"tools/cli\", \"crates/app\""), "{ws}");
        assert!(
            create(&root, "crates/app", "lib").is_err(),
            "an existing crate is refused"
        );
        assert_eq!(
            with_member("[workspace]\nmembers = [\"crates/*\"]\n", "crates/x"),
            None
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
