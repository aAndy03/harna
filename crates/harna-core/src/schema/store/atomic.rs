//! Component `atomic`: crash-safe writes for `.harna/` data.
//!
//! A bare `fs::write` truncates first and writes second — a crash, Ctrl-C,
//! full disk or AV lock in between leaves an empty or half-written
//! manifest that every later load refuses. Every `.harna/` write goes
//! through here instead: stage to a sibling temp file, flush it, then
//! rename over the live file (the same swap `migrate::restore` uses).
//! Readers see either the old bytes or the new bytes, never a mix.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;

/// Atomically replace `path` with `contents`.
pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> anyhow::Result<()> {
    let path = path.as_ref();
    let tmp = staging_path(path);
    let staged = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents.as_ref())?;
        f.sync_all()
    })();
    if let Err(e) = staged {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("staging {}", path.display()));
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("writing {}", path.display()));
    }
    Ok(())
}

/// Write `contents` to `path` only if nothing exists there yet. Records
/// that must append, never replace (decisions, archives) use this: a
/// collision is an error the caller resolves, not a silent overwrite.
pub fn write_new(path: &Path, contents: impl AsRef<[u8]>) -> anyhow::Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    f.write_all(contents.as_ref())
        .and_then(|_| f.sync_all())
        .with_context(|| format!("writing {}", path.display()))
}

/// First free `stem[_N].ext` in `dir` — the shared collision rule for
/// records that append (decisions, archives, score snapshots).
pub fn free_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut n = 0u32;
    loop {
        let name = if n == 0 {
            format!("{stem}{ext}")
        } else {
            format!("{stem}_{n}{ext}")
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Unique per call (start-time nanos + in-process counter): concurrent writers of the
/// same file — threads, or an MCP server beside a CLI — never share a
/// staging file; the last rename wins whole.
fn staging_path(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    // Per-process salt without touching `std::process` (schema is not the
    // process gateway): this process's first-write instant.
    static EPOCH: std::sync::LazyLock<u128> = std::sync::LazyLock::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    });
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "harna".into());
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(".{name}.tmp-{}-{seq}", *EPOCH))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("harna-atomic-{tag}-{:?}", std::thread::current().id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_replaces_and_leaves_no_staging_file() -> anyhow::Result<()> {
        let d = dir("replace");
        let p = d.join("manifest.toml");
        write(&p, "old")?;
        write(&p, "new")?;
        assert_eq!(fs::read_to_string(&p)?, "new");
        let names: Vec<_> = fs::read_dir(&d)?.flatten().map(|e| e.file_name()).collect();
        assert_eq!(names.len(), 1, "staging file must not survive: {names:?}");
        fs::remove_dir_all(&d)?;
        Ok(())
    }

    #[test]
    fn failed_write_keeps_the_old_bytes() -> anyhow::Result<()> {
        let d = dir("fail");
        let p = d.join("rules.toml");
        write(&p, "intact")?;
        // Unwritable bytes path: the live file is a directory-shaped
        // target the rename cannot replace, so the swap fails...
        let target = d.join("as-dir");
        fs::create_dir_all(target.join("child"))?;
        assert!(write(&target, "clobber").is_err());
        assert!(target.join("child").is_dir(), "...and the target is untouched");
        let names: Vec<_> = fs::read_dir(&d)?.flatten().map(|e| e.file_name()).collect();
        assert_eq!(names.len(), 2, "no staging file left behind: {names:?}");
        assert_eq!(fs::read_to_string(&p)?, "intact");
        fs::remove_dir_all(&d)?;
        Ok(())
    }

    #[test]
    fn concurrent_writers_of_one_file_never_collide() -> anyhow::Result<()> {
        let d = dir("concurrent");
        let p = d.join("rules.toml");
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let p = p.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        write(&p, format!("writer {i}")).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("no writer may fail");
        }
        assert!(fs::read_to_string(&p)?.starts_with("writer "), "a whole write wins");
        fs::remove_dir_all(&d)?;
        Ok(())
    }

    #[test]
    fn write_new_refuses_to_overwrite() -> anyhow::Result<()> {
        let d = dir("new");
        let p = d.join("decision.md");
        write_new(&p, "first")?;
        assert!(write_new(&p, "second").is_err());
        assert_eq!(fs::read_to_string(&p)?, "first");
        fs::remove_dir_all(&d)?;
        Ok(())
    }

    #[test]
    fn free_name_takes_the_next_suffix() -> anyhow::Result<()> {
        let d = dir("free");
        assert_eq!(free_name(&d, "x", ".md"), d.join("x.md"));
        fs::write(d.join("x.md"), "")?;
        fs::write(d.join("x_1.md"), "")?;
        assert_eq!(free_name(&d, "x", ".md"), d.join("x_2.md"));
        fs::remove_dir_all(&d)?;
        Ok(())
    }
}
