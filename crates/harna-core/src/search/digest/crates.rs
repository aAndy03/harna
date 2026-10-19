//! `crates` (0.3.44): the workspace's crates as Cargo declares them, not
//! as a folder convention guesses them.
//!
//! Before this index, a crate was "whatever sits before `/src/`" — so a
//! workspace whose crates keep `lib.rs` at the crate root (deno: 6 of ~90
//! crates have a `src/`) collapsed into one layer and lost almost every
//! cross-crate edge (36 edges for 1052 modules). The index reads:
//!
//! - `[workspace] members` (dirs and `dir/*` globs) plus the path
//!   dependencies the root declares (`[workspace.dependencies]`,
//!   `[dependencies]`) — Cargo makes those members implicitly;
//! - per crate: the package name, the lib target's name and path
//!   (`[lib] name`/`path`, default `src/lib.rs`), else the first binary;
//! - import aliases: the lib name and every `key = { package = "…" }`
//!   rename, so `use v8::…` resolves to the crate packaged as `deno_v8`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// One workspace crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateInfo {
    /// Repo-relative crate directory ("." for the root package).
    pub dir: String,
    /// `[package] name`.
    pub package: String,
    /// The directory module stems are relative to: the lib target's
    /// parent dir (`ext/fs` for `lib.rs` at the root, `crates/x/src`).
    pub src_root: String,
}

#[derive(Debug, Clone, Default)]
pub struct CrateIndex {
    /// Longest `src_root` first, so nested crates win over their parents.
    pub crates: Vec<CrateInfo>,
    /// Every name a `use` may start with (package, lib name, rename key,
    /// in their import spelling) -> the crate's package name.
    pub import_names: BTreeMap<String, String>,
}

impl CrateIndex {
    pub fn load(root: &Path) -> CrateIndex {
        let mut dirs: Vec<String> = Vec::new();
        let mut renames: Vec<(String, String)> = Vec::new();
        if let Some(v) = read_toml(&root.join("Cargo.toml")) {
            if v.get("package").is_some() {
                dirs.push(".".into());
            }
            let ws = v.get("workspace");
            for m in ws
                .and_then(|w| w.get("members"))
                .and_then(|m| m.as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| m.as_str())
            {
                expand_member(root, m, &mut dirs);
            }
            for table in [
                ws.and_then(|w| w.get("dependencies")),
                v.get("dependencies"),
            ]
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_table())
            {
                for (key, dep) in table {
                    if let Some(p) = dep.get("path").and_then(|p| p.as_str()) {
                        dirs.push(normalize(p));
                    }
                    if let Some(pkg) = dep.get("package").and_then(|p| p.as_str()) {
                        renames.push((key.clone(), pkg.to_string()));
                    }
                }
            }
        }
        dirs.sort();
        dirs.dedup();

        let mut index = CrateIndex::default();
        for dir in dirs {
            let base = if dir == "." {
                root.to_path_buf()
            } else {
                root.join(&dir)
            };
            let Some(v) = read_toml(&base.join("Cargo.toml")) else {
                continue;
            };
            let Some(package) = v
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
            else {
                continue; // a virtual manifest is not a crate
            };
            let lib = v.get("lib");
            let target = lib
                .and_then(|l| l.get("path"))
                .and_then(|p| p.as_str())
                .map(str::to_string)
                .or_else(|| {
                    base.join("src/lib.rs")
                        .is_file()
                        .then(|| "src/lib.rs".into())
                })
                .or_else(|| {
                    // A declared binary, else the first test/bench/example
                    // target: test-only crates (deno's tests/bench) declare
                    // nothing but `[[bench]] path = "main.rs"` (0.3.45).
                    ["bin", "test", "bench", "example"].iter().find_map(|k| {
                        v.get(*k)
                            .and_then(|b| b.as_array())
                            .and_then(|b| b.first())
                            .and_then(|b| b.get("path"))
                            .and_then(|p| p.as_str())
                            .map(str::to_string)
                    })
                })
                // A `src/` directory is the convention even before its
                // root file exists.
                .or_else(|| base.join("src").is_dir().then(|| "src/lib.rs".to_string()))
                .or_else(|| {
                    ["lib.rs", "main.rs"]
                        .iter()
                        .find(|f| base.join(f).is_file())
                        .map(|f| f.to_string())
                })
                // Nothing declared or found: the convention, as before —
                // a guess must never make one crate swallow the tree.
                .unwrap_or_else(|| "src/lib.rs".into());
            let target_dir = normalize(target.rsplit_once('/').map(|(d, _)| d).unwrap_or(""));
            let src_root = join(&dir, &target_dir);
            let lib_name = lib
                .and_then(|l| l.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or(package);
            for name in [package, lib_name] {
                index
                    .import_names
                    .insert(import_spelling(name), package.to_string());
            }
            index.crates.push(CrateInfo {
                dir,
                package: package.to_string(),
                src_root,
            });
        }
        for (key, pkg) in renames {
            if index.crates.iter().any(|c| c.package == pkg) {
                index.import_names.insert(import_spelling(&key), pkg);
            }
        }
        index.crates.sort_by(|a, b| {
            b.src_root
                .len()
                .cmp(&a.src_root.len())
                .then(a.dir.cmp(&b.dir))
        });
        index
    }

    /// The crate hosting `path` and the module stem relative to its source
    /// root. A file inside the crate dir but outside the source root
    /// (`tests/`, `benches/`, `build.rs`) belongs to the crate with a stem
    /// relative to the crate dir. None: no declared crate hosts the path.
    pub fn locate(&self, path: &str) -> Option<(&CrateInfo, String)> {
        let path = path.replace('\\', "/");
        if let Some(c) = self
            .crates
            .iter()
            .find(|c| c.src_root == "." || path.starts_with(&format!("{}/", c.src_root)))
        {
            let rel = if c.src_root == "." {
                path.as_str()
            } else {
                &path[c.src_root.len() + 1..]
            };
            return Some((c, stem_of(rel)));
        }
        let c = self
            .crates
            .iter()
            .filter(|c| c.dir == "." || path.starts_with(&format!("{}/", c.dir)))
            .max_by_key(|c| c.dir.len())?;
        let rel = if c.dir == "." {
            path.as_str()
        } else {
            &path[c.dir.len() + 1..]
        };
        Some((c, stem_of(rel)))
    }

    /// `(crate dir, stem)` for a file: the index's answer, else the
    /// `src/` convention — the one path-level entry point for callers
    /// that hold a path, not a map module.
    pub fn crate_of(&self, path: &str) -> (Option<String>, String) {
        match self.locate(path) {
            Some((c, stem)) => (Some(c.dir.clone()), stem),
            None => super::crate_of(path),
        }
    }

    /// A directory (a container path) inside a crate: the crate and the
    /// directory's prefix relative to the crate's source root ("" when the
    /// directory is the source root itself).
    pub fn locate_dir(&self, dir: &str) -> Option<(&CrateInfo, String)> {
        let dir = normalize(dir);
        if let Some(c) = self.crates.iter().find(|c| c.src_root == dir) {
            return Some((c, String::new()));
        }
        self.crates
            .iter()
            .find(|c| c.src_root == "." || dir.starts_with(&format!("{}/", c.src_root)))
            .map(|c| {
                let rel = if c.src_root == "." {
                    dir.clone()
                } else {
                    dir[c.src_root.len() + 1..].to_string()
                };
                (c, rel)
            })
    }

    /// Workspace crates `dir`'s normal `[dependencies]` reach (path deps and
    /// `workspace = true` ones), as crate dirs. One Cargo reading for adopt
    /// (which drafts `depends_on` from it) and `cargo-layering` (which
    /// checks it) — the draft must pass its own check (0.3.45).
    pub fn cargo_deps(&self, root: &Path, dir: &str) -> Vec<String> {
        let ws: BTreeMap<String, String> = read_toml(&root.join("Cargo.toml"))
            .and_then(|v| v.get("workspace")?.get("dependencies")?.as_table().cloned())
            .map(|t| {
                t.iter()
                    .filter_map(|(k, d)| Some((k.clone(), normalize(d.get("path")?.as_str()?))))
                    .collect()
            })
            .unwrap_or_default();
        let base = if dir == "." {
            root.to_path_buf()
        } else {
            root.join(dir)
        };
        let Some(deps) = read_toml(&base.join("Cargo.toml"))
            .and_then(|v| v.get("dependencies").and_then(|d| d.as_table()).cloned())
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (key, dep) in &deps {
            let target = if let Some(p) = dep.get("path").and_then(|p| p.as_str()) {
                Some(resolve_rel(dir, p))
            } else if dep.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
                ws.get(key).cloned()
            } else {
                None
            };
            if let Some(t) = target.filter(|t| self.crates.iter().any(|c| &c.dir == t)) {
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
        out
    }

    /// Import names as the resolver's crate list (each maps to a package).
    pub fn names(&self) -> Vec<String> {
        self.import_names.keys().cloned().collect()
    }

    /// Any import name or package name -> the crate's directory.
    pub fn dir_of(&self, name: &str) -> Option<&str> {
        let pkg = self
            .import_names
            .get(&import_spelling(name))
            .map(String::as_str)
            .unwrap_or(name);
        self.crates
            .iter()
            .find(|c| c.package == pkg)
            .map(|c| c.dir.as_str())
    }
}

fn expand_member(root: &Path, member: &str, out: &mut Vec<String>) {
    let member = normalize(member);
    let Some((before, after)) = member.split_once('*') else {
        out.push(member);
        return;
    };
    let before = before.trim_end_matches('/');
    let after = after.trim_start_matches('/');
    let Ok(entries) = fs::read_dir(root.join(before)) else {
        return;
    };
    for e in entries.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let dir = join(&join(before, &name), after);
        if root.join(&dir).join("Cargo.toml").is_file() {
            out.push(dir);
        }
    }
}

fn read_toml(path: &Path) -> Option<toml::Value> {
    toml::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn normalize(p: &str) -> String {
    let p = p.replace('\\', "/");
    let p = p.trim_start_matches("./").trim_end_matches('/');
    if p.is_empty() || p == "." {
        ".".into()
    } else {
        p.to_string()
    }
}

/// `dir` + a relative path with `..` folded: `cli` + `../libs/core` ->
/// `libs/core`.
fn resolve_rel(dir: &str, rel: &str) -> String {
    let joined = if dir == "." {
        rel.to_string()
    } else {
        format!("{dir}/{rel}")
    };
    let joined = joined.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

fn join(a: &str, b: &str) -> String {
    match (a, b) {
        (".", "") | ("", "") => ".".into(),
        (".", b) | ("", b) => normalize(b),
        (a, "") | (a, ".") => normalize(a),
        (a, b) => normalize(&format!("{a}/{b}")),
    }
}

/// Rust imports `a-b` as `a_b`.
fn import_spelling(name: &str) -> String {
    name.replace('-', "_")
}

fn stem_of(rel: &str) -> String {
    let s = rel.trim_end_matches(".rs");
    let s = s.trim_end_matches("/mod");
    s.to_string()
}

#[cfg(test)]
mod tests;
