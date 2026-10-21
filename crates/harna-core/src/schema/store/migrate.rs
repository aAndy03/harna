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

    // every register restamped by need (0.9.0: Default left them at 0, an
    // invalid version); a register that does not exist is not created
    macro_rules! register {
        ($name:expr, $load:ident, $save:ident) => {
            register!($name, $load, $save, |_x| crate::SCHEMA_BASELINE)
        };
        ($name:expr, $load:ident, $save:ident, $need:ident) => {
            register!($name, $load, $save, |x| x.$need())
        };
        ($name:expr, $load:ident, $save:ident, |$x:ident| $need:expr) => {
            if store_dir_path(store, $name)?.is_file() {
                reports.push(migrate_file(
                    store,
                    $name,
                    dry,
                    |s| {
                        s.$load().map(|x| {
                            let need: u32 = {
                                let $x = &x;
                                $need
                            };
                            (x.schema_version, need.max(crate::SCHEMA_BASELINE))
                        })
                    },
                    |s, v| {
                        let mut x = s.$load()?;
                        x.schema_version = v;
                        s.$save(&x)
                    },
                )?);
            }
        };
    }
    register!("templates.toml", load_templates, save_templates);
    register!("deferred.toml", load_deferred, save_deferred);
    register!("terms.toml", load_terms, save_terms);
    register!("non-goals.toml", load_non_goals, save_non_goals);
    register!("lessons.toml", load_lessons, save_lessons);
    register!("assumptions.toml", load_assumptions, save_assumptions);
    register!(
        "qualities.toml",
        load_qualities,
        save_qualities,
        required_schema
    );
    register!("roadmap.toml", load_roadmap, save_roadmap);
    register!("acknowledged.toml", load_acknowledged, save_acknowledged);
    register!("commands.toml", load_commands, save_commands);
    register!("baselines.toml", load_baselines, save_baselines);
    register!(
        "services.toml",
        load_services,
        save_services,
        required_schema
    );

    reports.push(migrate_file(
        store,
        "manifest.toml",
        dry,
        |s| {
            s.load_manifest()
                .map(|m| (m.schema_version, m.required_schema()))
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
        |s| {
            s.load_rules()
                .map(|r| (r.schema_version, r.required_schema()))
        },
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
            "harna migrate: data stamped with the schema it needs",
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
    // the target is what the data needs (0.4.9), never past what it uses
    let (version, target) = load(store)?;
    if version >= target {
        return Ok(MigrationReport {
            file: name.into(),
            from: version,
            to: version,
            action: "already-current".into(),
        });
    }
    if dry {
        return Ok(MigrationReport {
            file: name.into(),
            from: version,
            to: target,
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
    save(store, target)?;

    Ok(MigrationReport {
        file: name.into(),
        from: version,
        to: target,
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

/// Every TOML file under `.harna/` read back (0.9.0): typed loads for the
/// registers, and a parse plus a version check for each file of the
/// folders (parity events, contracts, plugins, scores). `archive/` and
/// `backup/` hold history and are not judged. Returns one line per problem;
/// empty = everything reads with this harna.
pub fn verify(store: &Store) -> Vec<String> {
    let mut problems = Vec::new();
    let root_dir = store.repo_root().join(".harna");
    let mut typed = |name: &str, r: anyhow::Result<Vec<String>>| {
        match r {
        Err(e) => problems.push(format!("{name}: {e:#}")),
        Ok(keys) if !keys.is_empty() => problems.push(format!(
            "{name}: harna does not know {} — the next save would drop it (a typo, or a newer harna's field)",
            keys.join(", ")
        )),
        Ok(_) => {}
    }
    };
    // a typed load, then the keys the model would drop
    macro_rules! keys {
        ($file:expr, $load:expr) => {
            $load.map(|v| {
                std::fs::read_to_string(root_dir.join($file))
                    .map(|raw| dropped_keys(&raw, &v))
                    .unwrap_or_default()
            })
        };
    }
    typed(
        "manifest.toml",
        keys!("manifest.toml", store.load_manifest()),
    );
    typed("rules.toml", keys!("rules.toml", store.load_rules()));
    typed(
        "templates.toml",
        keys!("templates.toml", store.load_templates()),
    );
    typed(
        "deferred.toml",
        keys!("deferred.toml", store.load_deferred()),
    );
    typed("terms.toml", keys!("terms.toml", store.load_terms()));
    typed(
        "non-goals.toml",
        keys!("non-goals.toml", store.load_non_goals()),
    );
    typed("lessons.toml", keys!("lessons.toml", store.load_lessons()));
    typed(
        "assumptions.toml",
        keys!("assumptions.toml", store.load_assumptions()),
    );
    typed(
        "qualities.toml",
        keys!("qualities.toml", store.load_qualities()),
    );
    typed("roadmap.toml", keys!("roadmap.toml", store.load_roadmap()));
    typed(
        "acknowledged.toml",
        keys!("acknowledged.toml", store.load_acknowledged()),
    );
    typed(
        "commands.toml",
        keys!("commands.toml", store.load_commands()),
    );
    typed(
        "baselines.toml",
        keys!("baselines.toml", store.load_baselines()),
    );
    typed(
        "services.toml",
        keys!("services.toml", store.load_services()),
    );
    typed("plugins/", store.load_plugins().map(|_| Vec::new()));
    typed(
        "handoff.toml",
        crate::schema::journal::handoff::try_load(store).map(|_| Vec::new()),
    );
    let root = root_dir;
    for dir in ["parity", "contracts", "scores"] {
        let Ok(entries) = fs::read_dir(root.join(dir)) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("toml") {
                continue;
            }
            let name = format!("{dir}/{}", e.file_name().to_string_lossy());
            let parsed = fs::read_to_string(&p)
                .map_err(anyhow::Error::from)
                .and_then(|raw| toml::from_str::<toml::Value>(&raw).map_err(anyhow::Error::from));
            match parsed {
                Err(e) => problems.push(format!("{name}: {e:#}")),
                Ok(v) => {
                    let found = v
                        .get("schema_version")
                        .and_then(|x| x.as_integer())
                        .unwrap_or(crate::SCHEMA_BASELINE as i64);
                    if found > crate::SCHEMA_VERSION as i64 {
                        problems.push(format!(
                            "{name}: schema v{found} — this harna reads up to v{}",
                            crate::SCHEMA_VERSION
                        ));
                    }
                }
            }
        }
    }
    problems
}

/// Keys a file holds that harna's model does not (0.9.0): a typo
/// (`rationle`), or a field a newer harna wrote without raising the
/// stamp. The next save would drop them silently; `verify` names them.
/// Empty values are skipped (harna omits those on save itself).
pub fn dropped_keys<T: serde::Serialize>(raw: &str, typed: &T) -> Vec<String> {
    let (Ok(raw), Ok(back)) = (
        toml::from_str::<toml::Value>(raw),
        toml::Value::try_from(typed),
    ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    diff(&raw, &back, "", &mut out);
    out
}

fn empty(v: &toml::Value) -> bool {
    match v {
        toml::Value::String(s) => s.is_empty(),
        toml::Value::Array(a) => a.is_empty(),
        toml::Value::Table(t) => t.is_empty(),
        // defaults a model leaves out on save (`isolated = false`)
        toml::Value::Boolean(b) => !b,
        toml::Value::Integer(i) => *i == 0,
        _ => false,
    }
}

fn diff(raw: &toml::Value, back: &toml::Value, at: &str, out: &mut Vec<String>) {
    match (raw, back) {
        (toml::Value::Table(r), toml::Value::Table(b)) => {
            for (k, v) in r {
                let here = if at.is_empty() {
                    k.clone()
                } else {
                    format!("{at}.{k}")
                };
                match b.get(k) {
                    Some(bv) => diff(v, bv, &here, out),
                    None if !empty(v) => out.push(here),
                    None => {}
                }
            }
        }
        (toml::Value::Array(r), toml::Value::Array(b)) => {
            for (i, (rv, bv)) in r.iter().zip(b).enumerate() {
                diff(rv, bv, &format!("{at}[{i}]"), out);
            }
        }
        _ => {}
    }
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
            &format!("schema_version = {}", crate::SCHEMA_BASELINE),
            "schema_version = 0",
        );
        fs::write(store.manifest_path(), &raw).unwrap();
        (tmp, store)
    }

    /// 0.4.9 review #4: data is stamped with the schema it needs. A plain
    /// manifest stays v1 (an older harna reads it); one holding a test kind
    /// is v2, so an older harna refuses it instead of stripping the kind.
    #[test]
    fn data_is_stamped_with_the_schema_it_needs() {
        let (tmp, store) = stale_store("stamp");
        let mut m = store.load_manifest().unwrap();
        store.save_manifest(&m).unwrap();
        assert_eq!(
            store.load_manifest().unwrap().schema_version,
            crate::SCHEMA_BASELINE
        );
        m.test_kinds.push(crate::schema::model::manifest::TestKind {
            name: "it".into(),
            paths: vec!["tests/**".into()],
            ..Default::default()
        });
        store.save_manifest(&m).unwrap();
        let raw = fs::read_to_string(store.manifest_path()).unwrap();
        assert!(raw.contains("schema_version = 2"), "{raw}");
        let mut r = store.load_rules().unwrap();
        r.rules.push(crate::schema::model::rules::Rule {
            id: "money".into(),
            scope_signature: Some("Money".into()),
            ..Default::default()
        });
        store.save_rules(&r).unwrap();
        assert_eq!(store.load_rules().unwrap().schema_version, 2);
        let _ = std::fs::remove_dir_all(&tmp);
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
            crate::SCHEMA_BASELINE
        );
        let backup = store_dir_path(&store, "backup")
            .unwrap()
            .join("manifest.toml.v0");
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
            crate::SCHEMA_BASELINE,
            "the live file survives a refused restore"
        );
        // A truncated (0-byte) backup is equally refused.
        fs::write(&backup, "   ").unwrap();
        assert!(restore(&store, "manifest.toml").is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 0.5.9 review H2: fields added after v2 shipped stamp v3, so a
    /// 0.4.9–0.5.8 harna (which knows v2) refuses them instead of
    /// stripping them.
    #[test]
    fn later_fields_stamp_v3() {
        let (tmp, store) = stale_store("v3");
        let mut m = store.load_manifest().unwrap();
        m.test_kinds.push(crate::schema::model::manifest::TestKind {
            name: "ui".into(),
            paths: vec!["tests/**".into()],
            contains: Some("x".into()),
            ..Default::default()
        });
        store.save_manifest(&m).unwrap();
        assert_eq!(store.load_manifest().unwrap().schema_version, 3);
        let mut q = store.load_qualities().unwrap();
        q.upsert(crate::schema::registers::qualities::Quality {
            id: "q".into(),
            statement: "s".into(),
            held_by: vec!["r".into()],
            ..Default::default()
        })
        .unwrap();
        store.save_qualities(&q).unwrap();
        let raw =
            fs::read_to_string(store.manifest_path().with_file_name("qualities.toml")).unwrap();
        assert!(raw.contains("schema_version = 3"), "{raw}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
