//! Migration: upgrade `.harna/` data to the schema version this tool speaks.
//!
//! Data is git-tracked, so a migration is a reviewable diff — harna never
//! mutates the repo's knowledge silently. Old files are backed up first and
//! every real migration is logged as a decision entry.

use std::fs;

use anyhow::Context as _;
use serde::Serialize;

use crate::schema::store::Store;
use crate::SCHEMA_VERSION;

/// One file's migration outcome.
#[derive(Debug, Clone, Serialize)]
pub struct MigrationReport {
    /// Repo-relative path of the migrated file.
    pub file: String,
    pub from: u32,
    pub to: u32,
    /// "already-current" or "migrated".
    pub action: String,
}

impl MigrationReport {
    pub fn render(&self) -> String {
        match self.action.as_str() {
            "migrated" => format!("{}: schema v{} -> v{} (backup kept in .harna/backup/)", self.file, self.from, self.to),
            _ => format!("{}: already at schema v{}", self.file, self.to),
        }
    }
}

/// Migrate every schema-bearing file. Future versions add per-step
/// transforms here; today the only versions are "has version field" = 1.
pub fn run(root: &std::path::Path, store: &Store) -> anyhow::Result<Vec<MigrationReport>> {
    let _ = root;
    let mut reports = Vec::new();

    reports.push(migrate_file(
        store,
        "manifest.toml",
        |s| s.load_manifest().map(|m| (m.schema_version, m.schema_version)),
        |s, v| {
            let mut m = s.load_manifest()?;
            m.schema_version = v;
            s.save_manifest(&m)
        },
    )?);

    reports.push(migrate_file(
        store,
        "rules.toml",
        |s| s.load_rules().map(|r| (r.schema_version, r.schema_version)),
        |s, v| {
            let mut r = s.load_rules()?;
            r.schema_version = v;
            s.save_rules(&r)
        },
    )?);

    let migrated: Vec<_> = reports.iter().filter(|r| r.action == "migrated").collect();
    if !migrated.is_empty() {
        let listing: Vec<String> = migrated
            .iter()
            .map(|r| format!("- {}: v{} -> v{}", r.file, r.from, r.to))
            .collect();
        store.save_decision(
            &format!("harna migrate: data schema -> v{SCHEMA_VERSION}"),
            &format!(
                "`harna migrate` upgraded .harna/ data files:\n{}\n\
                 Backups: .harna/backup/. Old harna versions may refuse to read these files.",
                listing.join("\n")
            ),
            None,
        )?;
    }

    Ok(reports)
}

/// Migrate one TOML file: read current version (bails on future versions via
/// store guards), back up, write with the new version stamp.
fn migrate_file<L, S>(
    store: &Store,
    name: &str,
    load: L,
    save: S,
) -> anyhow::Result<MigrationReport>
where
    L: FnOnce(&Store) -> anyhow::Result<(u32, u32)>,
    S: FnOnce(&Store, u32) -> anyhow::Result<()>,
{
    let path = store_dir_path(store, name)?;
    let (version, _same) = load(store)?;
    if version == SCHEMA_VERSION {
        return Ok(MigrationReport {
            file: name.into(),
            from: version,
            to: SCHEMA_VERSION,
            action: "already-current".into(),
        });
    }

    // Backup before any transform.
    let backup_dir = store_dir_path(store, "backup")?;
    fs::create_dir_all(&backup_dir)?;
    let backup = backup_dir.join(format!("{name}.v{version}"));
    fs::copy(&path, &backup).with_context(|| format!("backing up {name}"))?;

    // Future versions chain transforms here; v1 is the baseline.
    save(store, SCHEMA_VERSION)?;

    Ok(MigrationReport {
        file: name.into(),
        from: version,
        to: SCHEMA_VERSION,
        action: "migrated".into(),
    })
}

fn store_dir_path(store: &Store, name: &str) -> anyhow::Result<std::path::PathBuf> {
    Ok(store
        .manifest_path()
        .parent()
        .context("store has no parent")?
        .join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::store::Store;

    #[test]
    fn migrate_is_noop_when_current_and_creates_no_decision() {
        let tmp = std::env::temp_dir().join(format!("harna-mig-{}", std::process::id()));
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        let reports = run(&tmp, &store).unwrap();
        assert!(reports.iter().all(|r| r.action == "already-current"));
        assert_eq!(store.manifest_path().parent().unwrap().join("decisions").read_dir().unwrap().count(), 0);
    }

    #[test]
    fn future_schema_is_rejected_not_migrated() {
        let tmp = std::env::temp_dir().join(format!("harna-mig-future-{}", std::process::id()));
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        // Simulate an older tool: stamp a future version directly into the file.
        fs::write(store.manifest_path(), "schema_version = 999\n").unwrap();
        assert!(run(&tmp, &store).is_err(), "future schema must require a tool update");
        assert!(store.load_manifest().is_err());
    }
}
