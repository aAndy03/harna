//! Migration: upgrade `.harna/` data to the schema version this tool speaks.
//!
//! Data is git-tracked, so a migration is a reviewable diff — harna never
//! mutates the repo's knowledge silently. Old files are backed up first, the
//! backup is verified readable before any transform runs (0.3.24: a backup
//! that cannot be read back is not a backup), and every real migration is
//! logged as a decision entry. `migrate --dry` shows what WOULD move —
//! including the promised decision entry and the kept backup — and touches
//! nothing.

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
    /// "already-current", "would-migrate" (dry) or "migrated".
    pub action: String,
}

impl MigrationReport {
    pub fn render(&self) -> String {
        match self.action.as_str() {
            "would-migrate" => format!(
                "{}: WOULD migrate schema v{} -> v{} (backup would be kept in .harna/backup/, a decision would be recorded) — nothing written (--dry)",
                self.file, self.from, self.to
            ),
            "migrated" => format!(
                "{}: schema v{} -> v{} (backup kept in .harna/backup/, verified readable)",
                self.file, self.from, self.to
            ),
            _ => format!("{}: already at schema v{}", self.file, self.to),
        }
    }
}

/// Migrate every schema-bearing file. Future versions add per-step
/// transforms here; today the only versions are "has version field" = 1.
/// `dry` previews the run without writing anything.
pub fn run(
    root: &std::path::Path,
    store: &Store,
    dry: bool,
) -> anyhow::Result<Vec<MigrationReport>> {
    let _ = root;
    let mut reports = Vec::new();

    reports.push(migrate_file(
        store,
        "manifest.toml",
        dry,
        |s| {
            s.load_manifest()
                .map(|m| (m.schema_version, m.schema_version))
        },
        |s, v| {
            let mut m = s.load_manifest()?;
            m.schema_version = v;
            s.save_manifest(&m)
        },
    )?);

    reports.push(migrate_file(
        store,
        "rules.toml",
        dry,
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
                 Backups: .harna/backup/ (each verified readable before the \
                 transform touched the live file). Old harna versions may \
                 refuse to read these files.",
                listing.join("\n")
            ),
            None,
        )?;
    }

    Ok(reports)
}

/// Restore the newest backup of a data file over the live file. The backup
/// is parse-verified BEFORE the copy: restoring an unreadable backup would
/// exchange a broken file for a broken one. `name` must be a plain data
/// file name — no path separators (the backup dir is the only shelf this
/// reads, and this returns the restored file's repo-relative path).
pub fn restore(store: &Store, name: &str) -> anyhow::Result<String> {
    if name.contains('\\') || name.contains('/') || name.contains("..") || name.is_empty() {
        anyhow::bail!("restore takes a plain data-file name like `manifest.toml` (got `{name}`)");
    }
    let live = store_dir_path(store, name)?;
    if !live.is_file() {
        anyhow::bail!("`{name}` is not a live data file");
    }
    let backup_dir = store_dir_path(store, "backup")?;
    let mut candidates: Vec<(u32, std::path::PathBuf)> = backup_dir
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&format!("{name}.v")))
                .unwrap_or(false)
        })
        .filter_map(|p| {
            let v = p
                .file_name()?
                .to_str()?
                .rsplit_once(".v")?
                .1
                .parse::<u32>()
                .ok()?;
            Some((v, p))
        })
        .collect();
    candidates.sort();
    let Some((version, backup)) = candidates.pop() else {
        anyhow::bail!("no backup for `{name}` in .harna/backup/ — nothing to restore");
    };
    verify_backup(&backup)?;
    // Atomic replacement details: copy to a temp sibling, then rename over
    // the live file (a half-copied restore must never become the truth).
    let tmp = live.with_extension(format!("restore-tmp-{version}"));
    fs::copy(&backup, &tmp).with_context(|| format!("staging {backup:?}"))?;
    fs::rename(&tmp, &live).with_context(|| format!("swapping {name}"))?;
    Ok(format!(
        "{name}: restored v{version} from {backup:?} (verified readable before the swap)"
    ))
}

/// A backup must read back as TOML before anything depends on it — a
/// nonzero size and a parse are the difference between a copy and a backup.
fn verify_backup(backup: &std::path::Path) -> anyhow::Result<()> {
    let raw = fs::read_to_string(backup)
        .with_context(|| format!("reading backup {backup:?} — corrupted or unreadable"))?;
    if raw.trim().is_empty() {
        anyhow::bail!("backup {backup:?} is empty — not a usable backup");
    }
    toml::from_str::<serde::de::IgnoredAny>(&raw)
        .with_context(|| format!("backup {backup:?} does not parse as TOML"))
        .map(|_| ())
}

/// Migrate one TOML file: read current version (bails on future versions via
/// store guards), in dry mode report without writing; in real mode back up,
/// VERIFY the backup, then write with the new version stamp.
fn migrate_file<L, S>(
    store: &Store,
    name: &str,
    dry: bool,
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
    if dry {
        return Ok(MigrationReport {
            file: name.into(),
            from: version,
            to: SCHEMA_VERSION,
            action: "would-migrate".into(),
        });
    }

    // Backup before any transform, verified before the transform runs.
    let backup_dir = store_dir_path(store, "backup")?;
    fs::create_dir_all(&backup_dir)?;
    let backup = backup_dir.join(format!("{name}.v{version}"));
    fs::copy(&path, &backup).with_context(|| format!("backing up {name}"))?;
    verify_backup(&backup).with_context(|| {
        format!("{name}: the backup must be verifiably readable before migrating")
    })?;

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
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        let reports = run(&tmp, &store, false).unwrap();
        assert!(reports.iter().all(|r| r.action == "already-current"));
        assert_eq!(
            store
                .manifest_path()
                .parent()
                .unwrap()
                .join("decisions")
                .read_dir()
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn future_schema_is_rejected_not_migrated() {
        let tmp = std::env::temp_dir().join(format!("harna-mig-future-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        // Simulate an older tool: stamp a future version directly into the file.
        fs::write(store.manifest_path(), "schema_version = 999").unwrap();
        assert!(
            run(&tmp, &store, false).is_err(),
            "future schema must require a tool update"
        );
        assert!(store.load_manifest().is_err());
    }

    fn stale_store(id: &str) -> (std::path::PathBuf, Store) {
        let tmp = std::env::temp_dir().join(format!(
            "harna-mig-{id}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        // A repo from before the v1 stamping: manifest without schema_version
        // (serde default 0? no — 0 stamped by hand, the loaders accept it).
        let raw = fs::read_to_string(store.manifest_path()).unwrap();
        let raw = raw.replace(
            &format!("schema_version = {}", crate::SCHEMA_VERSION),
            "schema_version = 0",
        );
        fs::write(store.manifest_path(), &raw).unwrap();
        (tmp, store)
    }

    #[test]
    fn dry_touches_nothing_then_real_migrates_and_restores() {
        let (tmp, store) = stale_store("dry");
        let decisions = store.manifest_path().parent().unwrap().join("decisions");

        // Dry: honest preview, zero writes.
        let reports = run(&tmp, &store, true).unwrap();
        let would = reports
            .iter()
            .find(|r| r.action == "would-migrate")
            .unwrap();
        assert!(would.render().contains("WOULD migrate schema v0 -> v1"));
        assert!(would.render().contains("nothing written"));
        assert!(
            decisions.read_dir().unwrap().count() == 0,
            "dry records nothing"
        );
        assert!(
            store.load_manifest().unwrap().schema_version == 0,
            "dry migrates nothing"
        );
        assert_eq!(
            store_dir_path(&store, "backup")
                .unwrap()
                .read_dir()
                .map(|d| d.count())
                .unwrap_or(0),
            0,
            "dry backs up nothing"
        );

        // Real: backup verified, file stamped, decision recorded.
        let reports = run(&tmp, &store, false).unwrap();
        assert!(reports.iter().any(|r| r.action == "migrated"));
        assert_eq!(
            store.load_manifest().unwrap().schema_version,
            crate::SCHEMA_VERSION
        );
        let backup = store_dir_path(&store, "backup")
            .unwrap()
            .join(format!("manifest.toml.v0"));
        assert!(backup.is_file(), "the stale file is kept");
        assert_eq!(decisions.read_dir().unwrap().count(), 1);

        // Restore: newest backup verified BEFORE the swap, live file back to v0.
        let restored = restore(&store, "manifest.toml").unwrap();
        assert!(restored.contains("manifest.toml: restored v0"));
        assert!(store.load_manifest().unwrap().schema_version == 0);

        // .. traversal and missing backups are refused, never guessed.
        assert!(restore(&store, "no/such/../file.toml").is_err());
        assert!(
            restore(&store, "rules.toml").is_err(),
            "no backup exists for rules"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn corrupted_backup_refuses_to_restore() {
        let (tmp, store) = stale_store("corrupt");
        run(&tmp, &store, false).unwrap();
        // Corrupt the backup AFTER migration: a half-written shelf must not
        // become the truth because we asked for a restore.
        let backup = store_dir_path(&store, "backup")
            .unwrap()
            .join("manifest.toml.v0");
        fs::write(&backup, "not [ valid toml").unwrap();
        assert!(
            restore(&store, "manifest.toml").is_err(),
            "unreadable backup refused"
        );
        assert_eq!(
            store.load_manifest().unwrap().schema_version,
            crate::SCHEMA_VERSION,
            "the live file survives a refused restore"
        );
        // A truncated (0-byte) backup is equally refused.
        fs::write(&backup, "   ").unwrap();
        assert!(restore(&store, "manifest.toml").is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
