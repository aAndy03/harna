//! Component `views-sync` (1.1.0): every generated view in one pass, for
//! the CLI's `render`, the MCP `harna_render` and `harna finish` alike.
//! The CLI and the MCP tool each carried their own copy of this sequence,
//! and the MCP copy never refreshed the harna block in AGENTS.md.
//!
//! Order: per-scope instruction files, the harna block in agent
//! instruction files, CODEOWNERS, then ARCHITECTURE.md and SURFACE.md.

use std::path::Path;

use crate::schema::store::Store;

/// What one pass found: the views written (or, checking, the stale ones)
/// and notes worth showing.
#[derive(Debug, Default)]
pub struct Views {
    /// Written (write mode) or stale (check mode).
    pub changed: Vec<String>,
    pub notes: Vec<String>,
}

impl Views {
    pub fn render(&self, write: bool) -> String {
        let mut out: Vec<String> = if write {
            self.changed
                .iter()
                .map(|f| format!("rendered {f}"))
                .collect()
        } else {
            self.changed.iter().map(|f| format!("stale: {f}")).collect()
        };
        out.extend(self.notes.iter().cloned());
        if self.changed.is_empty() {
            out.push(if write {
                "generated views already current (nothing to write)".into()
            } else {
                "views are current".into()
            });
        }
        out.join("\n") + "\n"
    }
}

/// Bring every view up to date (`write`), or list the stale ones.
pub fn all(root: &Path, store: &Store, write: bool) -> anyhow::Result<Views> {
    let manifest = store.load_manifest()?;
    let rules = store.load_effective_rules()?;
    let mut v = Views::default();
    for (f, stale) in super::agents::scoped::sync(root, &manifest, &rules, write)? {
        if stale {
            v.changed.push(format!("the scoped block in {f}"));
        }
    }
    for (f, stale) in super::agents::sync(root, &manifest, &rules, &store.load_commands()?, write)?
    {
        if stale {
            v.changed.push(format!("the harna block in {f}"));
        }
    }
    {
        use super::codeowners::{sync, Owners};
        match sync(root, &manifest, write)? {
            // a write is a change like every other view's (review of
            // 1.1.2: it was reported as a note)
            Some(Owners::Stale) if !write => v.changed.push(".github/CODEOWNERS".into()),
            Some(Owners::Written) => v.changed.push(".github/CODEOWNERS".into()),
            Some(o) => v.notes.extend(o.note()),
            None => {}
        }
    }
    if manifest.is_empty() {
        v.notes
            .push("manifest has no layers — ARCHITECTURE.md waits for one".into());
        return Ok(v);
    }
    if super::render::is_stale(root, &manifest)? {
        if write {
            let p = super::render::write(root, &manifest)?;
            v.changed.push(rel(root, &p));
        } else {
            v.changed.push("ARCHITECTURE.md".into());
        }
    }
    match store.load_digest() {
        Ok(d) => {
            if super::render::surface_is_stale(root, &manifest, &d).unwrap_or(true) {
                if write {
                    let p = super::render::write_surface(root, &manifest, &d)?;
                    v.changed.push(rel(root, &p));
                } else {
                    v.changed.push("SURFACE.md".into());
                }
            }
        }
        Err(_) => v
            .notes
            .push("no code map yet — SURFACE.md needs one (`harna scan`)".into()),
    }
    Ok(v)
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}
