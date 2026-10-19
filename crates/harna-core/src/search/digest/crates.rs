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

use std::collections::{BTreeMap, BTreeSet};
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
    /// Workspace root directories ("." for the repo root's), deepest
    /// last — `workspace = true` resolves against the nearest one.
    pub workspaces: Vec<String>,
    /// Crate dirs the workspaces' `[workspace.dependencies]`/root deps point
    /// at — the canonical crate when two share a package name (0.3.46:
    /// zed's lint test fixture is also called `gpui`).
    pub canonical: Vec<String>,
    /// Repo-relative paths of every target Cargo declares by `path`
    /// (`[lib]`, `[[bin]]`, `[[test]]`, `[[bench]]`, `[[example]]`): roots
    /// of a module tree of their own (0.3.47, zed: `src/server.rs` libs).
    pub targets: BTreeSet<String>,
    /// Directories holding a `Cargo.toml` package no workspace lists
    /// (cargo-fuzz dirs, standalone examples, tools): their files sit inside
    /// a member's folder yet are not its modules (0.3.47).
    pub foreign: BTreeSet<String>,
}

impl CrateIndex {
    pub fn load(root: &Path) -> CrateIndex {
        let mut dirs: Vec<String> = Vec::new();
        let mut renames: Vec<(String, String)> = Vec::new();
        let mut canonical: Vec<String> = Vec::new();
        // The workspace roots: the repo root's manifest, or — when it has
        // no [workspace] (codex keeps its Rust in `codex-rs/` beside a Bazel
        // root) — every tracked Cargo.toml that declares one (0.3.46).
        let root_ws =
            read_toml(&root.join("Cargo.toml")).is_some_and(|v| v.get("workspace").is_some());
        let mut roots: Vec<String> = vec![".".into()];
        let manifests: Vec<String> =
            super::respected_files_where(root, 100_000, |p| p.ends_with("Cargo.toml"))
                .into_iter()
                .filter(|f| f == "Cargo.toml" || f.ends_with("/Cargo.toml"))
                .collect();
        if !root_ws {
            for f in &manifests {
                let dir = f.trim_end_matches("Cargo.toml").trim_end_matches('/');
                if dir.is_empty() {
                    continue;
                }
                if read_toml(&root.join(f)).is_some_and(|v| v.get("workspace").is_some()) {
                    roots.push(dir.to_string());
                }
            }
        }
        for wroot in &roots {
            let Some(v) = read_toml(&root.join(wroot).join("Cargo.toml")) else {
                continue;
            };
            if v.get("package").is_some() {
                dirs.push(normalize(wroot));
            }
            let ws = v.get("workspace");
            for m in ws
                .and_then(|w| w.get("members"))
                .and_then(|m| m.as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| m.as_str())
            {
                let mut found = Vec::new();
                expand_member(&root.join(wroot), m, &mut found);
                dirs.extend(found.into_iter().map(|d| join(wroot, &d)));
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
                        dirs.push(resolve_rel(wroot, p));
                        canonical.push(resolve_rel(wroot, p));
                    }
                    if let Some(pkg) = dep.get("package").and_then(|p| p.as_str()) {
                        renames.push((key.clone(), pkg.to_string()));
                    }
                }
            }
        }
        dirs.sort();
        dirs.dedup();

        let mut index = CrateIndex {
            workspaces: roots.clone(),
            canonical: canonical.clone(),
            ..CrateIndex::default()
        };
        // A member's path dependencies are crates Cargo builds too (0.3.47,
        // deno's `serde_v8/utilities`): the walk follows them.
        let mut seen: BTreeSet<String> = dirs.iter().cloned().collect();
        let mut queue: std::collections::VecDeque<String> = dirs.into();
        while let Some(dir) = queue.pop_front() {
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
            let declared = lib.and_then(|l| l.get("path")).into_iter().chain(
                ["bin", "test", "bench", "example"]
                    .iter()
                    .filter_map(|k| v.get(*k).and_then(|b| b.as_array()))
                    .flatten()
                    .filter_map(|b| b.get("path")),
            );
            for p in declared.filter_map(|p| p.as_str()) {
                index.targets.insert(resolve_rel(&dir, p));
            }
            for dep in path_deps(&v) {
                let d = resolve_rel(&dir, &dep);
                if seen.insert(d.clone()) {
                    queue.push_back(d);
                }
            }
            index.crates.push(CrateInfo {
                dir,
                package: package.to_string(),
                src_root,
            });
        }
        for f in &manifests {
            let dir = normalize(f.trim_end_matches("Cargo.toml").trim_end_matches('/'));
            if !index.crates.iter().any(|c| c.dir == dir) && !roots.contains(&dir) {
                index.foreign.insert(dir);
            }
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
    /// The deepest crate dir owns the file (0.3.47): `libs/core` keeps its
    /// `lib.rs` at the crate root, yet `libs/core/examples/snapshot/build.rs`
    /// belongs to the `snapshot` crate nested below it.
    pub fn locate(&self, path: &str) -> Option<(&CrateInfo, String)> {
        let path = path.replace('\\', "/");
        let under = |dir: &str| dir == "." || path.starts_with(&format!("{dir}/"));
        let owner = self
            .crates
            .iter()
            .filter(|c| under(&c.dir))
            .max_by_key(|c| c.dir.len())
            // a source root outside its crate dir (`[lib] path = "../…"`)
            .or_else(|| self.crates.iter().find(|c| under(&c.src_root)))?;
        let base = if under(&owner.src_root) {
            &owner.src_root
        } else {
            &owner.dir
        };
        let rel = if base == "." {
            path.as_str()
        } else {
            &path[base.len() + 1..]
        };
        Some((owner, stem_of(rel)))
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
        // The member's own workspace: the deepest root that contains it.
        let wroot = self
            .workspaces
            .iter()
            .filter(|w| w.as_str() == "." || dir == w.as_str() || dir.starts_with(&format!("{w}/")))
            .max_by_key(|w| w.len())
            .cloned()
            .unwrap_or_else(|| ".".into());
        let ws: BTreeMap<String, String> = read_toml(&root.join(&wroot).join("Cargo.toml"))
            .and_then(|v| v.get("workspace")?.get("dependencies")?.as_table().cloned())
            .map(|t| {
                t.iter()
                    .filter_map(|(k, d)| {
                        Some((k.clone(), resolve_rel(&wroot, d.get("path")?.as_str()?)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let base = if dir == "." {
            root.to_path_buf()
        } else {
            root.join(dir)
        };
        let Some(manifest) = read_toml(&base.join("Cargo.toml")) else {
            return Vec::new();
        };
        // `[dependencies]` plus every `[target.'cfg(…)'.dependencies]`
        // (zed's platform crates declare theirs per target) (0.3.46).
        let mut deps = manifest
            .get("dependencies")
            .and_then(|d| d.as_table())
            .cloned()
            .unwrap_or_default();
        for t in manifest
            .get("target")
            .and_then(|t| t.as_table())
            .into_iter()
            .flat_map(|t| t.values())
        {
            if let Some(d) = t.get("dependencies").and_then(|d| d.as_table()) {
                for (k, v) in d {
                    deps.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
        }
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

    /// The package names `dir` depends on that are not workspace crates
    /// (0.3.48, R2: who may name `reqwest`), from `[dependencies]`,
    /// `[build-dependencies]` and every target table; a rename
    /// (`http = { package = "reqwest" }`, also through
    /// `[workspace.dependencies]`) names its package. Dev-dependencies
    /// are the tests' business.
    pub fn external_deps(&self, root: &Path, dir: &str) -> Vec<String> {
        let base = if dir == "." {
            root.to_path_buf()
        } else {
            root.join(dir)
        };
        let Some(manifest) = read_toml(&base.join("Cargo.toml")) else {
            return Vec::new();
        };
        let wroot = self
            .workspaces
            .iter()
            .filter(|w| w.as_str() == "." || dir == w.as_str() || dir.starts_with(&format!("{w}/")))
            .max_by_key(|w| w.len())
            .cloned()
            .unwrap_or_else(|| ".".into());
        let ws = read_toml(&root.join(&wroot).join("Cargo.toml"))
            .and_then(|v| v.get("workspace")?.get("dependencies")?.as_table().cloned())
            .unwrap_or_default();
        let tables = |t: &toml::Value| {
            ["dependencies", "build-dependencies"]
                .into_iter()
                .filter_map(|k| t.get(k).and_then(|d| d.as_table()).cloned())
                .collect::<Vec<_>>()
        };
        let mut all = tables(&manifest);
        if let Some(targets) = manifest.get("target").and_then(|t| t.as_table()) {
            for t in targets.values() {
                all.extend(tables(t));
            }
        }
        let mut out = Vec::new();
        for (key, dep) in all.iter().flat_map(|t| t.iter()) {
            let inherited = dep.get("workspace").and_then(|w| w.as_bool()) == Some(true);
            let source = if inherited { ws.get(key) } else { Some(dep) };
            if source.and_then(|d| d.get("path")).is_some() {
                continue; // a workspace crate, not an external one
            }
            let package = source
                .and_then(|d| d.get("package"))
                .and_then(|p| p.as_str())
                .unwrap_or(key)
                .to_string();
            if !out.contains(&package) {
                out.push(package);
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
        // One package name, several crates: the one the workspace's own
        // dependency tables point at, else the shallowest directory.
        let mut same: Vec<&CrateInfo> = self.crates.iter().filter(|c| c.package == pkg).collect();
        same.sort_by_key(|c| {
            (
                !self.canonical.contains(&c.dir),
                c.dir.matches('/').count(),
                c.dir.clone(),
            )
        });
        same.first().map(|c| c.dir.as_str())
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

/// The `path = "…"` dependencies a manifest declares, in every table
/// Cargo builds from (normal, dev, build; target-specific too).
fn path_deps(v: &toml::Value) -> Vec<String> {
    let tables = |t: &toml::Value| {
        ["dependencies", "dev-dependencies", "build-dependencies"]
            .into_iter()
            .filter_map(|k| t.get(k).and_then(|d| d.as_table()))
            .flat_map(|d| d.values())
            .filter_map(|d| d.get("path").and_then(|p| p.as_str()).map(str::to_string))
            .collect::<Vec<_>>()
    };
    let mut out = tables(v);
    if let Some(targets) = v.get("target").and_then(|t| t.as_table()) {
        for t in targets.values() {
            out.extend(tables(t));
        }
    }
    out
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
pub(super) fn resolve_rel(dir: &str, rel: &str) -> String {
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
