//! Component `relocate` (0.3.39): `harna move <from> <to>` — restructure
//! without breaking the links.
//!
//! Moving a Rust file by hand breaks four things at once: the manifest
//! paths (component, tests, container), the rule scopes naming it, the
//! `mod` declaration in its old and new parent, and every `use` that
//! named its old module path. One move keeps all of them linked:
//! - the file moves, with its sibling folder of children (`x.rs` + `x/`);
//! - manifest and rules are rewritten through their typed models (never
//!   regex over TOML), every path equal to or under the old one;
//! - the old parent stops declaring it; `linkage::wire_module` declares
//!   it at the new place;
//! - `crate::old::path` (and `other_crate::old::path` from sibling
//!   crates) is rewritten across the workspace on token boundaries.
//! What text rewriting cannot see (`super::` paths, grouped
//! `use a::{old, …}` imports) the compiler names; the report says so.

use std::path::Path;

use crate::schema::manifest::{Component, Manifest};
use crate::schema::store::Store;

#[derive(Debug, Default)]
pub struct MoveReport {
    pub moved: Vec<String>,
    pub manifest_paths: usize,
    pub rule_scopes: usize,
    pub declaration_removed: Option<String>,
    pub wiring: String,
    pub imports: Vec<String>,
    pub module_path: Option<(String, String)>,
}

impl MoveReport {
    pub fn render(&self) -> String {
        let mut out = format!("moved {}\n", self.moved.join(" + "));
        out.push_str(&format!(
            "manifest: {} path(s) relinked · rules: {} scope(s) relinked\n",
            self.manifest_paths, self.rule_scopes
        ));
        if let Some(d) = &self.declaration_removed {
            out.push_str(&format!("module tree: removed `{d}`; {}\n", self.wiring));
        } else {
            out.push_str(&format!("module tree: {}\n", self.wiring));
        }
        if let Some((old, new)) = &self.module_path {
            out.push_str(&format!(
                "imports: `{old}` → `{new}` rewritten in {} file(s){}\n",
                self.imports.len(),
                if self.imports.is_empty() { String::new() } else { format!(" ({})", self.imports.join(", ")) }
            ));
            out.push_str("check: build now — `super::` paths and grouped `use …::{…}` imports are the compiler's to name\n");
        }
        out
    }
}

/// Rewrite `p` if it is `from` or under it (`from/…`).
fn relink(p: &str, from: &str, to: &str) -> Option<String> {
    if p == from {
        Some(to.to_string())
    } else {
        p.strip_prefix(&format!("{from}/")).map(|rest| format!("{to}/{rest}"))
    }
}

/// Every manifest path the move touches, relinked in place.
pub fn relink_manifest(m: &mut Manifest, pairs: &[(String, String)]) -> usize {
    fn comps(cs: &mut [Component], pairs: &[(String, String)], n: &mut usize) {
        for c in cs.iter_mut() {
            for (f, t) in pairs {
                if let Some(p) = relink(&c.path, f, t) {
                    c.path = p;
                    *n += 1;
                }
                for test in c.tests.iter_mut() {
                    if let Some(p) = relink(test, f, t) {
                        *test = p;
                        *n += 1;
                    }
                }
            }
            comps(&mut c.children, pairs, n);
        }
    }
    let mut n = 0;
    for layer in &mut m.layers {
        for c in &mut layer.containers {
            for (f, t) in pairs {
                if let Some(p) = relink(&c.path, f, t) {
                    c.path = p;
                    n += 1;
                }
            }
            comps(&mut c.components, pairs, &mut n);
        }
    }
    n
}

/// `crates/x/src/schema/terms_data.rs` → (`crates/x`, `schema::terms_data`).
fn module_path(file_rel: &str) -> Option<(String, String)> {
    let (dir, stem) = crate::search::digest::crate_of(file_rel);
    let stem = stem.trim_end_matches(".rs").to_string();
    Some((dir?, stem.replace('/', "::")))
}

/// Replace `old` with `new` where `old` is followed by a non-identifier
/// character (so `a::b` never rewrites `a::bc`).
fn replace_path(src: &str, old: &str, new: &str) -> Option<String> {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    let mut changed = false;
    while let Some(i) = rest.find(old) {
        let after = &rest[i + old.len()..];
        let boundary_after = !after.starts_with(|c: char| c.is_alphanumeric() || c == '_');
        let before = &rest[..i];
        let boundary_before = !before.ends_with(|c: char| c.is_alphanumeric() || c == '_');
        out.push_str(before);
        if boundary_after && boundary_before {
            out.push_str(new);
            changed = true;
        } else {
            out.push_str(old);
        }
        rest = after;
    }
    out.push_str(rest);
    changed.then_some(out)
}

/// The parent file that declares `file`'s module, and the module name.
fn declaring_parent(root: &Path, file_rel: &str) -> Option<(std::path::PathBuf, String)> {
    let file = root.join(file_rel);
    let (name, dir) = if file.file_name()? == "mod.rs" {
        let d = file.parent()?;
        (d.file_name()?.to_string_lossy().into_owned(), d.parent()?.to_path_buf())
    } else {
        (file.file_stem()?.to_string_lossy().into_owned(), file.parent()?.to_path_buf())
    };
    for cand in [dir.join("lib.rs"), dir.join("main.rs"), dir.with_extension("rs"), dir.join("mod.rs")] {
        if cand.is_file() && cand != file {
            return Some((cand, name));
        }
    }
    None
}

fn undeclare(parent: &Path, name: &str) -> anyhow::Result<Option<String>> {
    let src = std::fs::read_to_string(parent)?;
    let is_decl = |l: &str| {
        let t = l.trim();
        let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
        t == format!("mod {name};")
    };
    let Some(line) = src.lines().find(|l| is_decl(l)).map(str::to_string) else { return Ok(None) };
    let kept: Vec<&str> = src.lines().filter(|l| !is_decl(l)).collect();
    crate::schema::atomic::write(parent, kept.join("\n") + "\n")?;
    Ok(Some(line.trim().to_string()))
}

pub fn relocate(root: &Path, store: &Store, from: &str, to: &str) -> anyhow::Result<MoveReport> {
    let (from, to) = (from.trim_end_matches('/'), to.trim_end_matches('/'));
    for p in [from, to] {
        super::scaffold::check_inside_repo(p)?;
    }
    let src = root.join(from);
    if !src.exists() {
        anyhow::bail!("nothing to move: `{from}` does not exist");
    }
    if root.join(to).exists() {
        anyhow::bail!("`{to}` already exists — harna move never overwrites");
    }
    // A file's children folder travels with it (`x.rs` + `x/`).
    let mut pairs = vec![(from.to_string(), to.to_string())];
    if let (Some(fs), Some(ts)) = (from.strip_suffix(".rs"), to.strip_suffix(".rs")) {
        if root.join(fs).is_dir() {
            pairs.push((fs.to_string(), ts.to_string()));
        }
    }
    let mut report = MoveReport::default();
    // Rust bookkeeping before the bytes move (the old parent is found by
    // the old location).
    let is_rust = from.ends_with(".rs") || root.join(from).join("mod.rs").is_file();
    let decl_file = if from.ends_with(".rs") { from.to_string() } else { format!("{from}/mod.rs") };
    if is_rust {
        if let Some((parent, name)) = declaring_parent(root, &decl_file) {
            report.declaration_removed = undeclare(&parent, &name)?;
        }
    }
    for (f, t) in &pairs {
        let (a, b) = (root.join(f), root.join(t));
        if let Some(parent) = b.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&a, &b).map_err(|e| anyhow::anyhow!("moving `{f}` → `{t}`: {e}"))?;
        report.moved.push(format!("{f} → {t}"));
    }

    // Manifest + rule scopes, typed.
    if store.manifest_path().exists() {
        let mut m = store.load_manifest()?;
        report.manifest_paths = relink_manifest(&mut m, &pairs);
        if report.manifest_paths > 0 {
            store.save_manifest(&m)?;
        }
    }
    if store.rules_path().exists() {
        let mut rules = store.load_rules()?;
        for r in &mut rules.rules {
            for s in r.scope.iter_mut() {
                for (f, t) in &pairs {
                    if let Some(p) = relink(s, f, t) {
                        *s = p;
                        report.rule_scopes += 1;
                    }
                }
            }
        }
        if report.rule_scopes > 0 {
            store.save_rules(&rules)?;
        }
    }

    if is_rust {
        let new_decl = if to.ends_with(".rs") { to.to_string() } else { format!("{to}/mod.rs") };
        report.wiring = super::linkage::wire_module(root, &new_decl)?.render();
        if let (Some((cdir, old)), Some((ndir, new))) = (module_path(&decl_file), module_path(&new_decl)) {
            if cdir == ndir && old != new {
                report.imports = rewrite_imports(root, &cdir, &old, &new)?;
                report.module_path = Some((old, new));
            }
        }
    } else {
        report.wiring = "not a Rust module — nothing to wire".into();
    }
    Ok(report)
}

/// Rewrite `crate::old` in the crate itself and `<crate_ident>::old` in its
/// sibling crates. Returns the files changed.
fn rewrite_imports(root: &Path, crate_dir: &str, old: &str, new: &str) -> anyhow::Result<Vec<String>> {
    let names = crate::search::digest::crate_name_dirs(root, &[crate_dir.to_string()]);
    let ident = names.keys().next().map(|n| n.replace('-', "_"));
    let mut changed = Vec::new();
    for file in crate::search::digest::respected_rust_files(root, 100_000) {
        let path = root.join(&file);
        let Ok(src) = std::fs::read_to_string(&path) else { continue };
        let same_crate = file.starts_with(&format!("{crate_dir}/")) || crate_dir == ".";
        let mut text = src.clone();
        if same_crate {
            if let Some(t) = replace_path(&text, &format!("crate::{old}"), &format!("crate::{new}")) {
                text = t;
            }
        }
        if let Some(id) = &ident {
            if let Some(t) = replace_path(&text, &format!("{id}::{old}"), &format!("{id}::{new}")) {
                text = t;
            }
        }
        if text != src {
            crate::schema::atomic::write(&path, &text)?;
            changed.push(file);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests;
