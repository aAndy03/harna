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
//!
//! What text rewriting cannot see (`super::` paths, grouped
//! `use a::{old, …}` imports) the compiler names; the report says so.

use std::path::Path;

use crate::schema::model::manifest::{Component, Manifest};
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
                if self.imports.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", self.imports.join(", "))
                }
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
        p.strip_prefix(&format!("{from}/"))
            .map(|rest| format!("{to}/{rest}"))
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
fn module_path(root: &Path, file_rel: &str) -> Option<(String, String)> {
    let (dir, stem) = crate::search::digest::crates::CrateIndex::load(root).crate_of(file_rel);
    let stem = stem.trim_end_matches(".rs").to_string();
    Some((dir?, stem.replace('/', "::")))
}

/// Byte spans of string and char literals (plain, byte, and raw strings),
/// so a rewrite touches code and comments but never data: a test fixture
/// that spells an old path inside "…" keeps spelling it.
fn literal_spans(src: &str) -> Vec<(usize, usize)> {
    let b = src.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b'r' if matches!(b.get(i + 1), Some(b'"') | Some(b'#'))
                && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) =>
            {
                let start = i;
                let mut j = i + 1;
                let mut hashes = 0;
                while b.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if b.get(j) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                j += 1;
                let close: Vec<u8> = std::iter::once(b'"')
                    .chain(std::iter::repeat_n(b'#', hashes))
                    .collect();
                while j < b.len() && !b[j..].starts_with(&close) {
                    j += 1;
                }
                i = (j + close.len()).min(b.len());
                spans.push((start, i));
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(b.len());
                spans.push((start, i));
            }
            // A char literal like '"' (not a lifetime like 'a).
            b'\''
                if b.get(i + 2) == Some(&b'\'')
                    || (b.get(i + 1) == Some(&b'\\') && b.get(i + 3) == Some(&b'\'')) =>
            {
                let len = if b.get(i + 1) == Some(&b'\\') { 4 } else { 3 };
                spans.push((i, i + len));
                i += len;
            }
            _ => i += 1,
        }
    }
    spans
}

/// Replace `old` with `new` on identifier boundaries (so `a::b` never
/// rewrites `a::bc`), outside string and char literals.
fn replace_path(src: &str, old: &str, new: &str) -> Option<String> {
    let spans = literal_spans(src);
    let in_literal = |at: usize| spans.iter().any(|&(s, e)| at >= s && at < e);
    let mut out = String::with_capacity(src.len());
    let mut last = 0;
    let mut changed = false;
    let mut from = 0;
    while let Some(rel) = src[from..].find(old) {
        let i = from + rel;
        let after = &src[i + old.len()..];
        let boundary_after = !after.starts_with(|c: char| c.is_alphanumeric() || c == '_');
        let boundary_before = !src[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_');
        if boundary_after && boundary_before && !in_literal(i) {
            out.push_str(&src[last..i]);
            out.push_str(new);
            last = i + old.len();
            changed = true;
        }
        from = i + old.len();
    }
    out.push_str(&src[last..]);
    changed.then_some(out)
}

/// The parent file that declares `file`'s module, and the module name.
fn declaring_parent(root: &Path, file_rel: &str) -> Option<(std::path::PathBuf, String)> {
    let file = root.join(file_rel);
    let (name, dir) = if file.file_name()? == "mod.rs" {
        let d = file.parent()?;
        (
            d.file_name()?.to_string_lossy().into_owned(),
            d.parent()?.to_path_buf(),
        )
    } else {
        (
            file.file_stem()?.to_string_lossy().into_owned(),
            file.parent()?.to_path_buf(),
        )
    };
    for cand in [
        dir.join("lib.rs"),
        dir.join("main.rs"),
        dir.with_extension("rs"),
        dir.join("mod.rs"),
    ] {
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
        let t = t
            .strip_prefix("pub(crate) ")
            .or_else(|| t.strip_prefix("pub "))
            .unwrap_or(t);
        t == format!("mod {name};")
    };
    let Some(line) = src.lines().find(|l| is_decl(l)).map(str::to_string) else {
        return Ok(None);
    };
    let kept: Vec<&str> = src.lines().filter(|l| !is_decl(l)).collect();
    crate::schema::store::atomic::write(parent, kept.join("\n") + "\n")?;
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
    let decl_file = if from.ends_with(".rs") {
        from.to_string()
    } else {
        format!("{from}/mod.rs")
    };
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
        let new_decl = if to.ends_with(".rs") {
            to.to_string()
        } else {
            format!("{to}/mod.rs")
        };
        report.wiring = super::linkage::wire_module(root, &new_decl)?.render();
        if let (Some((cdir, old)), Some((ndir, new))) =
            (module_path(root, &decl_file), module_path(root, &new_decl))
        {
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
fn rewrite_imports(
    root: &Path,
    crate_dir: &str,
    old: &str,
    new: &str,
) -> anyhow::Result<Vec<String>> {
    let names = crate::search::digest::crate_name_dirs(root, &[crate_dir.to_string()]);
    let ident = names.keys().next().map(|n| n.replace('-', "_"));
    let mut changed = Vec::new();
    for file in crate::search::digest::respected_rust_files(root, 100_000) {
        let path = root.join(&file);
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let same_crate = file.starts_with(&format!("{crate_dir}/")) || crate_dir == ".";
        let mut text = src.clone();
        if same_crate {
            if let Some(t) = replace_path(&text, &format!("crate::{old}"), &format!("crate::{new}"))
            {
                text = t;
            }
        }
        if let Some(id) = &ident {
            if let Some(t) = replace_path(&text, &format!("{id}::{old}"), &format!("{id}::{new}")) {
                text = t;
            }
        }
        if text != src {
            crate::schema::store::atomic::write(&path, &text)?;
            changed.push(file);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests;
