//! Component `linkage` (0.3.38): a new file is linked into the build and
//! the manifest the moment it exists — no hand-wiring.
//!
//! Adding a Rust file means three edits that drift apart when done by
//! hand: the `mod` line in the parent module, the manifest component, and
//! that component's `tests`. `scaffold` already writes the component;
//! this module supplies the other two:
//! - `wire_module` walks from a new file up to its crate root (`lib.rs` /
//!   `main.rs`) and makes every level declare the next — creating a
//!   `mod.rs` for a fresh intermediate folder — so the file compiles and
//!   is reachable;
//! - `register_tests` fills empty `tests` fields with the tests that
//!   actually exist (inline `#[cfg(test)]`/`#[test]`, or a sibling
//!   `tests.rs`), which `scan` runs on every map rebuild.

use std::path::{Path, PathBuf};

use crate::schema::manifest::{Component, Manifest};

/// What wiring did: the declarations inserted (`file: pub mod x;`) and
/// the module files it had to create, or why it could not wire.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Wiring {
    pub declared: Vec<String>,
    pub created: Vec<String>,
    pub skipped: Option<String>,
}

impl Wiring {
    pub fn render(&self) -> String {
        if let Some(why) = &self.skipped {
            return format!("not wired into the module tree: {why}");
        }
        if self.declared.is_empty() {
            return "already wired into the module tree".into();
        }
        let mut out = format!("wired: {}", self.declared.join(", "));
        if !self.created.is_empty() {
            out.push_str(&format!(" (created {})", self.created.join(", ")));
        }
        out
    }
}

/// A Rust module name: what `mod x;` can resolve to `x.rs`.
pub fn is_module_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// kebab-case component names become module names (`user-repo` → `user_repo`).
pub fn module_name(name: &str) -> String {
    name.trim().replace('-', "_").to_lowercase()
}

/// Make `file_rel` reachable from its crate root, declaring each level in
/// its parent. Nothing is written unless a crate root exists above it.
pub fn wire_module(root: &Path, file_rel: &str) -> anyhow::Result<Wiring> {
    let file = root.join(file_rel);
    let Some(crate_root) = crate_root_above(root, &file) else {
        return Ok(Wiring {
            skipped: Some("no crate root (src/lib.rs or src/main.rs) above it".into()),
            ..Default::default()
        });
    };
    // (child module name, directory whose module must declare it)
    let (mut child, mut level) = match file.file_name().and_then(|n| n.to_str()) {
        Some("mod.rs") => {
            let dir = file.parent().unwrap_or(root);
            (dir_name(dir), dir.parent().unwrap_or(root).to_path_buf())
        }
        _ => (
            file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            file.parent().unwrap_or(root).to_path_buf(),
        ),
    };
    if crate_root.file == file {
        return Ok(Wiring::default()); // the crate root itself
    }
    let mut w = Wiring::default();
    loop {
        if !is_module_name(&child) {
            w.skipped = Some(format!("`{child}` is not a Rust module name (snake_case)"));
            return Ok(w);
        }
        let at_root = level == crate_root.dir;
        let parent = if at_root {
            crate_root.file.clone()
        } else if level.with_extension("rs").is_file() {
            level.with_extension("rs")
        } else {
            let m = level.join("mod.rs");
            if !m.is_file() {
                let header = format!("//! Module `{}`: its children are declared below.\n", dir_name(&level));
                crate::schema::atomic::write(&m, header)?;
                w.created.push(rel(root, &m));
            }
            m
        };
        if declare(&parent, &child)? {
            w.declared.push(format!("{}: pub mod {child};", rel(root, &parent)));
        }
        if at_root {
            return Ok(w);
        }
        child = dir_name(&level);
        level = level.parent().unwrap_or(root).to_path_buf();
    }
}

struct CrateRoot {
    dir: PathBuf,
    file: PathBuf,
}

/// The nearest directory above `file` holding `lib.rs` or `main.rs`,
/// without leaving the repository.
fn crate_root_above(root: &Path, file: &Path) -> Option<CrateRoot> {
    let mut dir = file.parent()?;
    loop {
        for name in ["lib.rs", "main.rs"] {
            let f = dir.join(name);
            if f.is_file() {
                return Some(CrateRoot { dir: dir.to_path_buf(), file: f });
            }
        }
        if dir == root {
            return None;
        }
        dir = dir.parent()?;
    }
}

/// Insert `pub mod child;` into `parent` unless it already declares it.
/// Placed after the last top-level module declaration (test modules
/// excluded), else after the leading `//!` docs. Returns whether it wrote.
fn declare(parent: &Path, child: &str) -> anyhow::Result<bool> {
    let src = std::fs::read_to_string(parent).unwrap_or_default();
    let declares = |l: &str| {
        let t = l.trim();
        let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
        t == format!("mod {child};") || t.starts_with(&format!("mod {child} "))
    };
    if src.lines().any(declares) {
        return Ok(false);
    }
    let mut out: Vec<String> = src.lines().map(str::to_string).collect();
    // A top-level, non-test `mod x;` line.
    let is_mod_decl = |i: usize, lines: &[String]| {
        let raw = &lines[i];
        let t = raw.trim_end();
        let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
        let test_attr = i > 0 && lines[i - 1].trim() == "#[cfg(test)]";
        !raw.starts_with(char::is_whitespace) && t.starts_with("mod ") && t.ends_with(';') && !test_attr
    };
    let decl = format!("pub mod {child};");
    match (0..out.len()).rev().find(|&i| is_mod_decl(i, &out)) {
        // Beside its siblings.
        Some(i) => out.insert(i + 1, decl),
        None => {
            // After the `//!` header (one blank line between), else on top.
            let docs = out.iter().take_while(|l| l.starts_with("//!")).count();
            if docs > 0 {
                out.insert(docs, String::new());
                out.insert(docs + 1, decl);
            } else {
                out.insert(0, decl);
            }
        }
    }
    crate::schema::atomic::write(parent, out.join("\n") + "\n")?;
    Ok(true)
}

fn dir_name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// Where a component's tests live, if anywhere: inline in its own file, or
/// in the sibling `tests.rs` its `#[cfg(test)] mod tests;` points at.
pub fn tests_of(root: &Path, path: &str) -> Option<String> {
    let src = std::fs::read_to_string(root.join(path)).ok()?;
    let inline = src.lines().any(|l| {
        let t = l.trim();
        t == "#[test]" || t.starts_with("#[test]") || t == "mod tests {"
    });
    if inline {
        return Some(path.to_string());
    }
    let out_of_line = src.lines().any(|l| l.trim() == "mod tests;");
    if out_of_line {
        let stem = path.strip_suffix("/mod.rs").or_else(|| path.strip_suffix(".rs"))?;
        let sibling = format!("{stem}/tests.rs");
        if root.join(&sibling).is_file() {
            return Some(sibling);
        }
    }
    None
}

/// Fill every empty `tests` field whose component has real tests. Returns
/// the component names it registered (the manifest is mutated in place).
pub fn register_tests(root: &Path, manifest: &mut Manifest) -> Vec<String> {
    fn walk(root: &Path, comps: &mut [Component], out: &mut Vec<String>) {
        for c in comps.iter_mut() {
            if c.tests.is_empty() && c.path.ends_with(".rs") {
                if let Some(t) = tests_of(root, &c.path) {
                    c.tests = vec![t];
                    out.push(c.name.clone());
                }
            }
            walk(root, &mut c.children, out);
        }
    }
    let mut out = Vec::new();
    for layer in &mut manifest.layers {
        for container in &mut layer.containers {
            walk(root, &mut container.components, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests;
