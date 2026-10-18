//! Digest: the map's parsing and query engine.
//!
//! The agent queries the digest instead of rescanning thousands of files.
//! Only signatures and structure are stored — never file bodies — so the
//! map stays small enough to inline into a prompt or diff in a PR.
//!
//! The DATA structs (Digest/Module/Item, EXTRACTOR_VERSION) live in the
//! memory layer (`schema/digest_data.rs`) — the store persists them, and
//! search may reach memory without the reverse being true (0.3.9 ended
//! the memory↔search layer cycle by moving data home). The names are
//! re-exported here so consumers keep their import paths.

use std::fs;
use std::path::Path;

// The data model is owned by the memory layer; re-exported for all
// existing consumers.
pub use crate::schema::map::Digest;
pub use crate::schema::map::Item;
pub use crate::schema::map::Module;
pub use crate::schema::map::EXTRACTOR_VERSION;

mod extract;
mod resolve;
pub use extract::*;
pub use resolve::*;
// The pub(super) helpers, for this module and its tests.
#[allow(unused_imports)]
use extract::*;
#[allow(unused_imports)]
use resolve::*;

impl Item {
    fn parse(line: &str) -> Option<Item> {
        let t = line.trim();
        let (vis, rest) = if let Some(rest) = t.strip_prefix("pub ") {
            ("pub", rest)
        } else if t.starts_with("pub(") {
            // pub(crate) / pub(in path) — still a cross-module boundary ("pub");
            // keep the '(' so the paren-strip below can remove the whole lid
            ("pub", &t[3..])
        } else {
            ("priv", t)
        };
        let (vis, rest) = if vis == "pub" && rest.starts_with('(') {
            // the visibility lid ends at the FIRST ')' — the parameter list
            // after the name also closes with ')'
            ("pub", rest.split_once(')')?.1.trim_start())
        } else {
            (vis, rest)
        };
        // `async fn` is its own inventory kind (`async-fn`, 0.3.25): the
        // role gates read the async family without re-parsing source text.
        let is_async = rest.starts_with("async ");
        let rest = if is_async { &rest[6..] } else { rest };
        for kind in [
            "impl", "fn", "struct", "enum", "trait", "const", "static", "type", "mod",
        ] {
            let prefix = format!("{kind} ");
            let Some(rest2) = rest.strip_prefix(&prefix) else {
                continue;
            };
            let rest3 = rest2.trim_start();
            let mut name = String::new();
            for c in rest3.trim_start().chars() {
                if c.is_alphanumeric() || c == '_' {
                    name.push(c);
                } else {
                    break;
                }
            }
            if name.is_empty() {
                continue;
            }
            return Some(Item {
                vis: vis.into(),
                kind: if is_async {
                    "async-fn".into()
                } else {
                    kind.into()
                },
                name,
                text: t.trim_end().to_string(),
            });
        }
        None
    }
}

impl Digest {
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    /// Total signature count across modules — a rough "size of the API".
    pub fn signature_count(&self) -> usize {
        self.modules.iter().map(|m| m.signatures.len()).sum()
    }

    /// Render a token-efficient overview for agents.
    pub fn render_overview(&self, max_modules: usize) -> String {
        let first = &self.modules[..self.modules.len().min(max_modules)];
        render_overview_from(
            &self.generated_at,
            self.module_count(),
            self.signature_count(),
            first,
            max_modules,
        )
    }

    /// The codebase's own hierarchy, rendered: a directory tree of the
    /// modules with their public-surface and inventory sizes.
    pub fn render_tree(&self) -> String {
        use std::collections::BTreeMap;
        #[derive(Default)]
        struct Node {
            dirs: BTreeMap<String, Node>,
            files: Vec<(String, usize, usize)>, // (name, pub sigs, items)
        }
        fn walk(node: &Node, prefix: &str, out: &mut String) {
            for (name, child) in &node.dirs {
                out.push_str(&format!("{prefix}{name}/\n"));
                walk(child, &format!("{prefix}    "), out);
            }
            for (name, sigs, items) in &node.files {
                out.push_str(&format!("{prefix}{name}  ({sigs} pub, {items} items)\n"));
            }
        }
        let mut root = Node::default();
        for m in &self.modules {
            let parts: Vec<&str> = m.path.split('/').collect();
            let mut cur = &mut root;
            for (i, part) in parts.iter().enumerate() {
                if i == parts.len() - 1 {
                    cur.files
                        .push((part.to_string(), m.signatures.len(), m.inventory.len()));
                } else {
                    cur = cur.dirs.entry(part.to_string()).or_default();
                }
            }
        }
        let mut s = format!("codebase tree ({} modules)\n", self.modules.len());
        walk(&root, "", &mut s);
        s
    }
}

/// The overview rendered from counts plus only the leading modules
/// (0.3.16): the bounded-onboarding shape. Header counts come from the
/// streaming pass or the index — not from a materialized map.
/// `Digest::render_overview` delegates here; the text is identical both
/// ways.
pub fn render_overview_from(
    generated_at: &str,
    module_count: usize,
    signature_count: usize,
    first: &[Module],
    max_modules: usize,
) -> String {
    let mut out = format!(
        "codebase map: {} modules, {} signatures (generated {})\n",
        module_count, signature_count, generated_at
    );
    for m in first {
        out.push_str(&format!("- {} ({})\n", m.path, m.lang));
        for sig in m.signatures.iter().take(5) {
            out.push_str(&format!("    {}\n", sig));
        }
        if m.signatures.len() > 5 {
            out.push_str(&format!("    … +{} more\n", m.signatures.len() - 5));
        }
    }
    if module_count > max_modules {
        out.push_str(&format!("… +{} more modules\n", module_count - max_modules));
    }
    out
}

/// Scan a repository and build a digest.
///
/// Full build: every respected `.rs` file re-parsed. See `build_incremental`
/// for the mtime-backed reuse path.
pub fn build(root: &Path, max_files: usize) -> anyhow::Result<Digest> {
    build_incremental(root, max_files, None, true).map(|(d, _, _)| d)
}

/// Incremental build: the respect-file list defines what exists; a module
/// is **reused** from the previous map when its re-hashed body matches the
/// stored hash — only changed files are re-parsed. Hash-verify, never
/// mtime-gate: wall-clock ticks lie (mtime and stamp can share a timer
/// tick), content hashes do not. `full = true` disables reuse.
///
/// Returns the digest plus (reused, rescanned) counts — the benchmark the
/// caller renders.
pub fn build_incremental(
    root: &Path,
    max_files: usize,
    previous: Option<&Digest>,
    full: bool,
) -> anyhow::Result<(Digest, usize, usize)> {
    use std::collections::HashMap;
    let crate_names = workspace_crate_names(root);
    let files = respected_rust_files(root, max_files);
    let previous_map: HashMap<&str, &Module> = previous
        .map(|p| p.modules.iter().map(|m| (m.path.as_str(), m)).collect())
        .unwrap_or_default();
    // Reuse is hash-verified AND extractor-verified: a map stamped by an
    // older extractor still carries its old dep edges, and the file hashes
    // say nothing about that — the whole map must be rebuilt.
    let reuse_trusted = !full && previous.is_some_and(|p| p.extractor_version == EXTRACTOR_VERSION);

    // Parallel scan (0.3.17): the per-file work (hash-verified reuse or a
    // fresh parse) is embarrassingly parallel — one thread per chunk of
    // the file list, and the final path sort below makes the result
    // identical whichever way the chunks landed. Small repos stay on the
    // calling thread: spawning costs more than parsing a dozen files.
    const MIN_FILES_PER_THREAD: usize = 16;
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(files.len().div_ceil(MIN_FILES_PER_THREAD).max(1));
    let mut modules: Vec<Module> = Vec::with_capacity(files.len());
    let mut reused = 0usize;
    if threads <= 1 {
        let (mut chunk_modules, chunk_reused) =
            scan_chunk(&files, root, &crate_names, &previous_map, reuse_trusted);
        modules.append(&mut chunk_modules);
        reused = chunk_reused;
    } else {
        let chunk_size = files.len().div_ceil(threads);
        // Shared references reborrowed once: the worker closures copy the
        // reference, not the data behind it.
        let crate_names_ref: &[String] = &crate_names;
        let previous_map_ref = &previous_map;
        std::thread::scope(|s| -> anyhow::Result<()> {
            let mut handles = Vec::new();
            for chunk in files.chunks(chunk_size) {
                handles.push(s.spawn(move || {
                    scan_chunk(
                        chunk,
                        root,
                        crate_names_ref,
                        previous_map_ref,
                        reuse_trusted,
                    )
                }));
            }
            for h in handles {
                let (mut chunk_modules, chunk_reused) = h
                    .join()
                    .map_err(|_| anyhow::anyhow!("scan worker panicked"))?;
                modules.append(&mut chunk_modules);
                reused += chunk_reused;
            }
            Ok(())
        })?;
    }
    let rescanned = modules.len() - reused;
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    modules.truncate(max_files);
    Ok((
        Digest {
            generated_at: now(),
            extractor_version: EXTRACTOR_VERSION,
            modules,
        },
        reused,
        rescanned,
    ))
}

/// One chunk of the scan: hash-verified reuse when trusted, a fresh parse
/// otherwise. Pure over its inputs — the unit the parallel scan fans out.
fn scan_chunk(
    chunk: &[String],
    root: &Path,
    crate_names: &[String],
    previous_map: &std::collections::HashMap<&str, &Module>,
    reuse_trusted: bool,
) -> (Vec<Module>, usize) {
    let mut modules = Vec::with_capacity(chunk.len());
    let mut reused = 0usize;
    for rel in chunk {
        let abs = root.join(rel);
        if reuse_trusted {
            if let Some(prev) = previous_map.get(rel.as_str()) {
                if !prev.body_hash.is_empty() {
                    let unchanged = fs::read_to_string(&abs)
                        .map(|src| hash_body(src.as_bytes()) == prev.body_hash)
                        .unwrap_or(false);
                    if unchanged {
                        modules.push((*prev).clone());
                        reused += 1;
                        continue;
                    }
                }
            }
        }
        modules.push(scan_rust_file(root, &abs, crate_names));
    }
    (modules, reused)
}

/// The `.rs` files the project "respects": tracked + untracked-not-ignored
/// when git is present (so `.gitignore`, `.git/info/exclude` and global
/// excludes rule the scan), else the internal skip-list walk.
pub fn respected_rust_files(root: &Path, max_files: usize) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    let output = std::process::Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .current_dir(root)
        .output();
    if let Ok(out) = output {
        if out.status.success() {
            for entry in String::from_utf8_lossy(&out.stdout).split('\0') {
                if entry.is_empty() || !entry.ends_with(".rs") {
                    continue;
                }
                if files.len() >= max_files {
                    break;
                }
                if root.join(entry).is_file() {
                    files.push(entry.replace('\\', "/"));
                }
            }
            return files;
        }
    }
    // No git / git failed: walk with the built-in skip list.
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == ".harna" || name == "target" || name == "node_modules" || name == ".git"
                {
                    continue;
                }
                stack.push(path);
            } else if files.len() < max_files
                && path.extension().and_then(|e| e.to_str()) == Some("rs")
            {
                files.push(
                    path.strip_prefix(root)
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default(),
                );
            }
        }
    }
    files.sort();
    files
}

/// Which workspace crate does a module path live in, and what is its
/// crate-relative module stem? `src/x.rs` -> root crate, stem `x`.
pub fn crate_of(module_path: &str) -> (Option<String> /*crate dir*/, String /*stem*/) {
    if let Some((before, tail)) = module_path.split_once("/src/") {
        (Some(before.to_string()), stem_of_rel(tail))
    } else if let Some(tail) = module_path.strip_prefix("src/") {
        (Some(".".into()), stem_of_rel(tail))
    } else {
        (None, stem_of_rel(module_path))
    }
}

fn stem_of_rel(rel: &str) -> String {
    let s = rel.replace('\\', "/");
    let s = s.trim_end_matches(".rs");
    let s = s.trim_end_matches("/mod");
    s.trim_end_matches(".rs").to_string()
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests;
