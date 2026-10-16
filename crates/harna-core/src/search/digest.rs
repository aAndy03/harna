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

use std::collections::HashMap;
use std::fs;
use std::path::Path;

// The data model is owned by the memory layer; re-exported for all
// existing consumers.
pub use crate::schema::digest_data::EXTRACTOR_VERSION;
pub use crate::schema::digest_data::Item;
pub use crate::schema::digest_data::Module;
pub use crate::schema::digest_data::Digest;
use crate::schema::manifest::Manifest;

impl Item {
    fn parse(line: &str) -> Option<Item> {
        let t = line.trim();
        let (vis, rest) = if t.starts_with("pub ") {
            ("pub", &t[4..])
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
        for kind in ["impl", "fn", "struct", "enum", "trait", "const", "static", "type", "mod"] {
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
                kind: kind.into(),
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
        let mut out = format!(
            "codebase map: {} modules, {} signatures (generated {})\n",
            self.module_count(),
            self.signature_count(),
            self.generated_at
        );
        for m in self.modules.iter().take(max_modules) {
            out.push_str(&format!("- {} ({})\n", m.path, m.lang));
            for sig in m.signatures.iter().take(5) {
                out.push_str(&format!("    {}\n", sig));
            }
            if m.signatures.len() > 5 {
                out.push_str(&format!("    … +{} more\n", m.signatures.len() - 5));
            }
        }
        if self.modules.len() > max_modules {
            out.push_str(&format!("… +{} more modules\n", self.modules.len() - max_modules));
        }
        out
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
    let reuse_trusted = !full
        && previous.map_or(false, |p| p.extractor_version == EXTRACTOR_VERSION);

    let mut modules = Vec::new();
    let mut reused = 0usize;
    for rel in &files {
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
        modules.push(scan_rust_file(root, &abs, &crate_names));
    }
    let rescanned = modules.len() - reused;
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    modules.truncate(max_files);
    Ok((Digest { generated_at: now(), extractor_version: EXTRACTOR_VERSION, modules }, reused, rescanned))
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

/// Names of crates declared in the workspace — `use <name>::`
/// statements in member crates are internal deps, not external ones.
/// Public so the parity ledger can resolve cross-crate import edges.
pub fn workspace_crate_names(root: &Path) -> Vec<String> {
    let Ok(manifest) = fs::read_to_string(root.join("Cargo.toml")) else {
        return Vec::new();
    };
    let Ok(value) = toml::from_str::<toml::Value>(&manifest) else {
        return Vec::new();
    };
    let Some(members) = value
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for member in members {
        let Some(s) = member.as_str() else { continue };
        collect_member_crate_names(root, s, &mut names);
    }
    names
}

/// Expand one workspace member (dir path or glob like `crates/*`) to the
/// package name(s) declared in its Cargo.toml.
fn collect_member_crate_names(root: &Path, member: &str, out: &mut Vec<String>) {
    if member.contains('*') {
        let (before, after) = member
            .split_once('*')
            .map(|(b, a)| (b.to_string(), a.to_string()))
            .unwrap_or_else(|| (member.into(), String::new()));
        if let Ok(dirs) = fs::read_dir(root.join(&before)) {
            for entry in dirs.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let rest = after.trim_start_matches(['/', '\\']);
                let cargo = entry.path().join(rest).join("Cargo.toml");
                push_crate_name(&cargo, out);
            }
        }
    } else {
        push_crate_name(&root.join(member).join("Cargo.toml"), out);
    }
}

fn push_crate_name(cargo: &Path, out: &mut Vec<String>) {
    if let Ok(src) = fs::read_to_string(cargo) {
        if let Ok(v) = toml::from_str::<toml::Value>(&src) {
            if let Some(name) = v
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
            {
                out.push(name.to_string());
            }
        }
    }
}

/// Where does an internal dep id point?
///
/// - `crate::a::b` → same crate, stem `a/b`
/// - `super::x` in module stem `a/b` → same crate, stem `a/x` (crate root
///   `super::` is not a valid reference; unresolved)
/// - `self::x` in module stem `a/b` → same crate, stem `a/b/x`
/// - `<workspace-crate>::a::b` → that crate, stem `a/b`
///
/// The resolved stem is a *module stem prefix*; symbols beyond it
/// (`schema::store::Store`) are allowed detail.
#[derive(Debug, Clone, PartialEq)]
pub struct DepTarget {
    /// Some(crate name) for cross-crate edges; None for same-crate edges.
    pub external: Option<String>,
    pub stem: String,
}

pub fn resolve_dep(
    dep: &str,
    importer_stem: &str,
    crate_names: &[String],
) -> Option<DepTarget> {
    let importer = importer_stem.trim_end_matches("/mod");
    if let Some(rest) = dep.strip_prefix("crate::") {
        return Some(DepTarget { external: None, stem: stem_of_path(rest) });
    }
    if let Some(rest) = dep.strip_prefix("super::") {
        let parent = importer.split('/').collect::<Vec<_>>();
        if parent.len() <= 1 {
            // crate-root file: super:: is invalid Rust; nothing to resolve
            return None;
        }
        let parent = parent[..parent.len() - 1].join("/");
        let body = stem_of_path(rest);
        let stem = if parent.is_empty() {
            if body.is_empty() {
                return None;
            }
            body
        } else if body.is_empty() {
            parent
        } else {
            format!("{parent}/{body}")
        };
        return Some(DepTarget { external: None, stem });
    }
    if let Some(rest) = dep.strip_prefix("self::") {
        if importer.is_empty() {
            return None;
        }
        let body = stem_of_path(rest);
        let stem = if body.is_empty() {
            importer.to_string()
        } else {
            format!("{importer}/{body}")
        };
        return Some(DepTarget { external: None, stem });
    }
    let head = dep.split("::").next().unwrap_or("");
    if !head.is_empty() && crate_names.iter().any(|n| crate_name_eq(n, head)) {
        let rest = dep.split_once("::").map(|(_, r)| r).unwrap_or("");
        return Some(DepTarget {
            external: Some(head.to_string()),
            stem: stem_of_path(rest),
        });
    }
    None
}

/// Rust normalizes crate-name hyphens to underscores at import sites
/// (`harna-core` is imported as `harna_core`); compare both spellings equal.
pub fn crate_name_eq(a: &str, b: &str) -> bool {
    a.replace('-', "_") == b.replace('-', "_")
}

/// Workspace crate package name -> its directory, read from each crate
/// dir's Cargo.toml (honest toml parse, not line-grepping). One canonical
/// read: ranks, cycles and budgets must never diverge on which package a
/// directory carries. Entries with unreadable or unparseable Cargo.toml
/// are skipped (nothing to be consistent about).
pub fn crate_name_dirs(root: &Path, dirs: &[String]) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    for dir in dirs {
        let Ok(src) = fs::read_to_string(root.join(dir).join("Cargo.toml")) else {
            continue;
        };
        let Ok(value) = toml::from_str::<toml::Value>(&src) else {
            continue;
        };
        let Some(pkg) = value
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
        else {
            continue;
        };
        map.insert(pkg.to_string(), dir.clone());
    }
    map
}

/// The deepest digest module that hosts a resolved dep target (longest
/// hosting stem wins when module stems nest). External targets map through
/// the crate package-name → dir index built over the digest's own crate
/// dirs (`crate_name_dirs`); same-crate targets stay inside the importer's
/// crate dir (unset dir counts as the root crate "."). None: the target
/// resolves outside the map (unhosted stems are each consumer's policy).
pub fn hosted_module<'a>(
    digest: &'a Digest,
    name_dirs: &std::collections::BTreeMap<String, String>,
    importer_dir: Option<&str>,
    target: &DepTarget,
) -> Option<&'a Module> {
    let importer_dir = importer_dir.unwrap_or(".");
    let target_dir = match target.external.as_deref() {
        Some(name) => name_dirs.get(name)?,
        None => importer_dir,
    };
    digest
        .modules
        .iter()
        .filter(|m| {
            let (dir, stem) = crate_of(&m.path);
            dir.as_deref() == Some(target_dir)
                && (stem == target.stem || target.stem.starts_with(&format!("{stem}/")))
        })
        .max_by_key(|m| crate_of(&m.path).1.len())
}

/// A digest module's home: the container whose path prefixes the module
/// (longest container path wins when containers nest) and that
/// container's layer. Moved here from `validate` in 0.3.12 so the metrics
/// view (search layer) shares the one home resolution instead of copying
/// it — layering, transit corridors, gateway `base`, and metrics can never
/// disagree about who hosts a module.
pub struct ModuleHome<'m> {
    pub layer: &'m str,
    pub container_path: &'m str,
}

/// digest module path -> its home. Modules outside every container stay
/// unhomed (the map does not reach them — same adoption discipline as
/// the gateway check's ungoverned carve-out).
pub fn module_homes<'m>(
    manifest: &'m Manifest,
    digest: &'m Digest,
) -> HashMap<&'m str, ModuleHome<'m>> {
    let mut hosts: Vec<(&str, &str)> = manifest
        .layers
        .iter()
        .flat_map(|l| l.containers.iter().map(|c| (c.path.as_str(), l.name.as_str())))
        .collect();
    hosts.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
    let mut map = HashMap::new();
    for m in &digest.modules {
        if let Some((path, layer)) = hosts.iter().find(|(path, _)| {
            !path.is_empty() && m.path.starts_with(&format!("{path}/"))
        }) {
            map.insert(
                m.path.as_str(),
                ModuleHome { layer, container_path: path },
            );
        }
    }
    map
}

/// digest module path -> owning layer name. The longest container path
/// wins when containers nest (the same prefix rule applied everywhere the
/// manifest prefixes the map). Derived from `module_homes`.
pub fn module_layers<'m>(
    manifest: &'m Manifest,
    digest: &'m Digest,
) -> HashMap<&'m str, &'m str> {
    module_homes(manifest, digest)
        .into_iter()
        .map(|(k, home)| (k, home.layer))
        .collect()
}

fn stem_of_path(s: &str) -> String {
    s.split("::")
        .filter(|x| !x.is_empty() && *x != "*")
        .collect::<Vec<_>>()
        .join("/")
}

/// Does internal dep `dep` of module `importer_stem` reach candidate module
/// `module_path`? The graph semantics shared by every consumer (parity
/// fan-out, layer boundaries, dependents queries): resolution from
/// `resolve_dep`, then stem-prefix matching against the candidate's stem.
pub fn dep_points_at(
    importer_stem: &str,
    dep: &str,
    module_path: &str,
    crate_names: &[String],
) -> bool {
    let stem = crate_of(module_path).1;
    if stem.is_empty() {
        return false;
    }
    let Some(target) = resolve_dep(dep, importer_stem, crate_names) else {
        return false;
    };
    if target.stem.is_empty() {
        return false;
    }
    let segs: Vec<&str> = stem.split('/').collect();
    let _ = segs;
    // The dep reaches the candidate when their module stems meet exactly,
    // or when one is inside the other's module boundary:
    // - dep deeper (candidate file, symbols suffixing it), or
    // - candidate deeper (dep names an ancestor module that disseminates it).
    let candidate_border = format!("{stem}/");
    let dep_border = format!("{}/", target.stem);
    if target.stem == stem || target.stem.starts_with(&candidate_border) || stem.starts_with(&dep_border) {
        return true;
    }
    false
}

/// Extract a module's internal dependency list.
///
/// Two sources, both test-excluded:/// - `use` statements (`crate::a::b::{X, Y}` collapses to `crate::a::b`)
/// - inline path references in expressions/calls, e.g. `crate::a::b::f()`
///   or `harna_core::schema::store::Store` — so an importer that never
///   declares a `use` still receives propagation fan-out.
/// Dep ids keep the raw path shape; only `crate::`/`super::`/`self::` and
/// workspace-crate imports count (std and external crates excluded).
pub fn extract_deps(
    src: &str,
    crate_names: &[String],
    sibling_mods: &[String],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("mod tests") {
            break;
        }
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }

        // 1) use statements
        if let Some(rest) = trimmed.trim_end_matches(';').strip_prefix("use ") {
            let head = rest
                .split('{')
                .next()
                .unwrap_or(rest)
                .trim()
                .trim_end_matches("::")
                .to_string();
            if !head.is_empty() && head.contains("::") {
                let first_segment = head.split("::").next().unwrap_or("");
                let rooted = head.starts_with("crate::")
                    || head.starts_with("super::")
                    || head.starts_with("self::")
                    || crate_names.iter().any(|n| crate_name_eq(n, first_segment));
                if rooted && !out.contains(&head) {
                    out.push(head);
                }
            }
        }

        // 2) inline path references
        for dep in inline_crate_refs(trimmed, crate_names) {
            if !out.contains(&dep) {
                out.push(dep);
            }
        }

        // 3) bare sibling-module paths in expression position: a file may
        // reach its own declared submodules without any rooted prefix
        // (`rule_bindings::Bindings` from validate's mod.rs) — legal Rust,
        // and exactly what only-rooted extraction missed, making such
        // importers invisible to parity fan-out, dependents and whereis.
        // `use` lines cannot be bare sibling paths in Rust 2018+, so they
        // are exempt from this rule.
        if !trimmed.starts_with("use ") {
            for dep in inline_sibling_refs(trimmed, sibling_mods) {
                if !out.contains(&dep) {
                    out.push(dep);
                }
            }
        }
    }
    out
}

/// Collect bare sibling references (`sibling::item`) as `self::sibling::item`
/// dependency ids — resolvable against the importer's own module stem.
fn inline_sibling_refs(line: &str, sibling_mods: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for m in sibling_mods {
        let needle = format!("{m}::");
        let mut from = 0usize;
        while let Some(pos) = line[from..].find(needle.as_str()) {
            let start = from + pos + needle.len();
            let coll = &line[start.min(line.len())..];
            let end = coll
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(coll.len());
            let segments: Vec<&str> = std::iter::once(m.as_str())
                .chain(coll[..end].split("::").filter(|s| !s.is_empty() && *s != "*"))
                .collect();
            if segments.len() > 1 {
                let dep = format!("self::{}", segments.join("::"));
                if !out.contains(&dep) {
                    out.push(dep);
                }
            }
            from = start + end;
            if from >= line.len() {
                break;
            }
        }
    }
    out
}

/// Collect internal path references appearing anywhere in a line
/// (`crate::a::b::c()`, `my_crate::x.y()`), as dependency ids.
fn inline_crate_refs(line: &str, crate_names: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut needles: Vec<(String, String)> =
        vec![("crate".to_string(), "crate::".to_string())];
    for name in crate_names {
        needles.push((name.clone(), format!("{name}::")));
        // import sites normalize hyphens to underscores; cover both spellings
        let underscored = name.replace('-', "_");
        if underscored != *name {
            needles.push((name.clone(), format!("{underscored}::")));
        }
    }
    for (name, needle) in &needles {
        let mut from = 0usize;
        while let Some(pos) = line[from..].find(needle.as_str()) {
            let start = from + pos + needle.len();
            let coll = &line[start.min(line.len())..];
            let end = coll
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(coll.len());
            let segments: Vec<&str> = coll[..end]
                .split("::")
                .filter(|s| !s.is_empty() && *s != "*")
                .collect();
            if !segments.is_empty() {
                let dep = format!("{name}::{}", segments.join("::"));
                if !out.contains(&dep) {
                    out.push(dep);
                }
            }
            from = start + end;
            if from >= line.len() {
                break;
            }
        }
    }
    out
}

fn scan_rust_file(root: &Path, path: &Path, crate_names: &[String]) -> Module {
    let rel = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let mut signatures = Vec::new();
    let mut inventory: Vec<Item> = Vec::new();
    let mut mods: Vec<String> = Vec::new();
    let mut deps: Vec<String> = Vec::new();
    let mut body_hash = String::new();
    if let Ok(src) = fs::read_to_string(path) {
        body_hash = hash_body(src.as_bytes());
        // Test sections are fixtures, not shipped surface: their items must
        // not appear as signatures (same rule as validate and deps).
        let mut in_test_code = false;
        for line in src.lines() {
            let trimmed = line.trim();
            if !in_test_code
                && (trimmed.starts_with("#[cfg(test)]")
                    || trimmed.starts_with("mod tests")
                    || trimmed.starts_with("#[test]"))
            {
                in_test_code = true;
            }
            if in_test_code || trimmed.starts_with("//") || trimmed.starts_with("///")
                || trimmed.starts_with("//!") {
                continue;
            }
            if let Some(item) = Item::parse(trimmed) {
                if item.kind == "mod" {
                    mods.push(item.name.clone());
                }
                inventory.push(item);
            }
            let pub_rest = line.trim_start().strip_prefix("pub ").unwrap_or("");
            // a visibility lid like pub(crate) ends at its own ')' — the
            // parameter list after the name closes with its own ')'.
            let pub_rest = if line.trim_start().starts_with("pub(") {
                line.trim_start().split_once(')').map(|(_, r)| r.trim_start()).unwrap_or("")
            } else {
                pub_rest
            };
            if pub_rest.starts_with("fn ")
                || pub_rest.starts_with("struct ")
                || pub_rest.starts_with("enum ")
                || pub_rest.starts_with("trait ")
            {
                signatures.push(trimmed.trim_end().to_string());
            }
        }
        mods.sort();
        deps = extract_deps(&src, crate_names, &mods);
    }
    Module { path: rel, lang: "rust".into(), signatures, deps, body_hash, inventory, mods }
}

/// Content hash of a module body; lets `harna diff` observe body-only changes.
fn hash_body(bytes: &[u8]) -> String {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Submodule names declared in a module body (comment lines and test
/// sections excluded) — the same list the scanner records as `mods`, exposed
/// so the boundary check can reproduce the sibling edges the map carries.
pub fn declared_mods(src: &str) -> Vec<String> {
    let mut mods = Vec::new();
    let mut in_test_code = false;
    for line in src.lines() {
        let trimmed = line.trim();
        if !in_test_code
            && (trimmed.starts_with("#[cfg(test)]")
                || trimmed.starts_with("#[test]")
                || trimmed.starts_with("mod tests"))
        {
            in_test_code = true;
        }
        if in_test_code || trimmed.starts_with("//") || trimmed.starts_with("///")
            || trimmed.starts_with("//!") {
            continue;
        }
        if let Some(item) = Item::parse(trimmed) {
            if item.kind == "mod" {
                mods.push(item.name);
            }
        }
    }
    mods.sort();
    mods.dedup();
    mods
}

/// Declared item names (pub and private, comments and test sections
/// excluded) — the same names the inventory catalogs, exposed so the
/// pattern rules police exactly the names the map knows.
pub fn declared_names(src: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_test_code = false;
    for line in src.lines() {
        let trimmed = line.trim();
        if !in_test_code
            && (trimmed.starts_with("#[cfg(test)]")
                || trimmed.starts_with("#[test]")
                || trimmed.starts_with("mod tests"))
        {
            in_test_code = true;
        }
        if in_test_code || trimmed.starts_with("//") || trimmed.starts_with("///")
            || trimmed.starts_with("//!") {
            continue;
        }
        if let Some(item) = Item::parse(trimmed) {
            names.push(item.name);
        }
    }
    names.sort();
    names.dedup();
    names
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
mod tests {
    use super::*;

    #[test]
    fn extract_deps_internal_only() {
        let src = "\
use std::fs;
use serde::Serialize;
use crate::schema::store::Store;
use crate::schema::store::{Store, StoreOpen};
use super::digest::Digest;
use self::helpers::x;
use harna_core::schema::manifest::Manifest;
pub fn main() {}
";
        let names = vec!["harna_core".to_string()];
        let deps = extract_deps(src, &names, &[]);
        assert!(deps.contains(&"crate::schema::store::Store".to_string()));
        // brace-group import collapses to its head module path
        assert!(deps.contains(&"crate::schema::store".to_string()));
        assert!(deps.contains(&"super::digest::Digest".to_string()));
        assert!(deps.contains(&"self::helpers::x".to_string()));
        assert!(deps.contains(&"harna_core::schema::manifest::Manifest".to_string()));
        // std and external crates are not internal deps
        assert!(!deps.contains(&"std::fs".to_string()));
        assert!(!deps.contains(&"serde::Serialize".to_string()));
    }

    #[test]
    fn extract_deps_inline_references_count() {
        let src = "\
fn run(store: &crate::schema::store::Store) -> u32 {
    harna_core::schema::manifest::count(store)
}
let url = fetch(\"https://example.com\");
// crate::docs::not_a_dep
";
        let names = vec!["harna_core".to_string()];
        let deps = extract_deps(src, &names, &[]);
        assert!(deps.contains(&"crate::schema::store::Store".to_string()));
        assert!(deps.contains(&"harna_core::schema::manifest::count".to_string()));
        // comment references are not deps
        assert!(!deps.contains(&"crate::docs::not_a_dep".to_string()));
    }

    #[test]
    fn extract_deps_bare_sibling_references_count() {
        let src = "\
use crate::schema::store::Store;
mod rules {
    pub fn r() {}
}
pub mod payloads;
fn go() { rules::r(); payloads::walk(); }
use rules::ghost;
";
        let deps = extract_deps(src, &["harna_core".to_string()], &declared_mods(src));
        assert!(
            deps.contains(&"self::rules::r".to_string()),
            "sibling call site must become an edge"
        );
        assert!(deps.contains(&"self::payloads::walk".to_string()));
        // `use` statements cannot be bare sibling paths in Rust 2018+ —
        // the exemption keeps that channel from inventing edges.
        assert!(!deps.contains(&"self::rules::ghost".to_string()));
    }

    #[test]
    fn extract_deps_skips_test_sections() {
        let src = "use crate::schema::store::Store;\n\
#[cfg(test)]\nmod tests {\n    use crate::fixture::OnlyFixture;\n}";
        let deps = extract_deps(src, &[], &[]);
        assert!(deps.contains(&"crate::schema::store::Store".to_string()));
        assert!(!deps.contains(&"crate::fixture::OnlyFixture".to_string()));
    }

    #[test]
    fn scan_respects_gitignore_and_falls_back_without_git() {
        let root = std::env::temp_dir().join(format!("harna-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("generated")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        std::fs::write(root.join("src/real.rs"), "pub fn real() {}").unwrap();
        std::fs::write(root.join("generated/noise.rs"), "pub fn noise() {}").unwrap();

        // With git: generated/ is ignored -> invisible to the map.
        std::fs::write(root.join(".gitignore"), "/generated/\n").unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .output()
            .unwrap();
        let d = build(&root, 100).unwrap();
        assert!(d.modules.iter().any(|m| m.path.contains("real.rs")));
        assert!(
            !d.modules.iter().any(|m| m.path.contains("noise.rs")),
            ".gitignore must rule the scan"
        );

        // Without git: fallback walk (same scope as the old skip-list behavior).
        std::fs::remove_dir_all(root.join(".git")).unwrap();
        let d2 = build(&root, 100).unwrap();
        assert!(d2.modules.iter().any(|m| m.path.contains("noise.rs")));
    }

    #[test]
    fn incremental_scan_reuses_unchanged_and_rescans_changed() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!("harna-incr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        // Both files predate the first scan: their mtimes are older than the
        // baseline stamp, so one full pass is enough to set up the reuse test.
        std::fs::write(root.join("src/stable.rs"), "pub fn stable() {}").unwrap();
        std::fs::write(root.join("src/moving.rs"), "pub fn first() {}").unwrap();

        let (first, reused0, scanned0) = build_incremental(&root, 100, None, false)?;
        assert_eq!(reused0, 0);
        assert_eq!(scanned0, 2);

        // Baseline is stamped "now"; changing moving.rs after the scan moves
        // its mtime past the stamp -> it must be rescanned. stable.rs stays.
        std::fs::write(root.join("src/moving.rs"), "pub fn second() {}").unwrap();

        let (second, reused, scanned) = build_incremental(&root, 100, Some(&first), false)?;
        assert!(reused == 1, "stable.rs reused, got reused={reused} scanned={scanned}");
        assert_eq!(second.extractor_version, EXTRACTOR_VERSION);
        second
            .modules
            .iter()
            .find(|m| m.path.ends_with("moving.rs"))
            .filter(|m| m.signatures.iter().any(|s| s.contains("second")))
            .expect("changed module must be rescanned");
        second
            .modules
            .iter()
            .find(|m| m.path.ends_with("stable.rs"))
            .filter(|m| m.signatures.iter().any(|s| s.contains("stable")))
            .expect("reused or rescanned, content must be intact");

        // Hash-verified reuse is deterministic: stable reused, moving rescanned.
        assert_eq!(reused, 1);
        assert_eq!(scanned, 1);
        assert_eq!(reused + scanned, second.modules.len());

        // Full rebuild ignores reuse entirely and still agrees.
        let (third, _, _) = build_incremental(&root, 100, Some(&second), true)?;
        assert_eq!(third.modules.len(), second.modules.len());
        assert!(third
            .modules
            .iter()
            .any(|m| m.signatures.iter().any(|s| s.contains("second"))));
        Ok(())
    }

    #[test]
    fn extractor_upgrade_forces_a_rebuild() -> anyhow::Result<()> {
        // A map whose extractor_version belongs to an older harna must not
        // be reused module by module — the file hashes cannot know that the
        // extractor gained new dep shapes.
        let root = std::env::temp_dir().join(format!("harna-extr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        std::fs::write(root.join("src/a.rs"), "pub fn a() {}").unwrap();
        let (fresh, reused0, _) = build_incremental(&root, 100, None, false)?;
        assert_eq!(reused0, 0);

        let mut stale = fresh.clone();
        stale.extractor_version = 0;
        let (rebuilt, reused, _) = build_incremental(&root, 100, Some(&stale), false)?;
        assert_eq!(reused, 0, "stale-extractor maps rebuild in full");
        assert_eq!(rebuilt.extractor_version, EXTRACTOR_VERSION);
        Ok(())
    }

    #[test]
    fn test_fixtures_do_not_pollute_signatures() {
        // scan_rust_file path exercised indirectly: build() on a temp crate.
        let root = std::env::temp_dir().join(format!("harna-sigfix-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/m.rs"),
            "pub fn real() {}\n\n#[cfg(test)]\nmod tests {\n    \
pub fn phantom() {}\n}",
        )
        .unwrap();
        let d = build(&root, 100).unwrap();
        let m = d.modules.iter().find(|m| m.path == "src/m.rs").unwrap();
        assert_eq!(m.signatures, vec!["pub fn real() {}".to_string()]);
    }

    #[test]
    fn renders_overview_with_truncation() {
        let d = Digest {
            generated_at: "now".into(),
            extractor_version: EXTRACTOR_VERSION,
            modules: vec![Module {
                path: "src/lib.rs".into(),
                lang: "rust".into(),
                signatures: (0..9).map(|i| format!("pub fn f{i}()")).collect(),
                deps: vec![],
                body_hash: String::new(),
                inventory: vec![],
                mods: vec![],
            }],
        };
        let text = d.render_overview(5);
        assert!(text.contains("src/lib.rs"));
        assert!(text.contains("+4 more")); // 9 sigs, first 5 shown

        let tree = d.render_tree();
        assert!(tree.contains("lib.rs  (9 pub, 0 items)"));
    }

    #[test]
    fn parse_pub_crate() {
        use super::*;
        let it = Item::parse("pub(crate) fn crate_visible() {}");
        println!("PARSE: {it:?}");
        assert!(it.is_some());
    }

    #[test]
    fn inventory_captures_visibility_and_kinds() {
        let root = std::env::temp_dir().join(format!("harna-inv-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/m.rs"),
            "pub struct Api;
fn helper() {}
pub(crate) fn crate_visible() {}
impl Default for Api {
    fn default() -> Self { Self }
}
",
        )
        .unwrap();
        let d = build(&root, 10).unwrap();
        let m = d.modules.iter().find(|m| m.path == "src/m.rs").unwrap();
        for i in &m.inventory { println!("INV {} {} {}", i.vis, i.kind, i.name); }
        let kinds: Vec<(&str, &str)> =
            m.inventory.iter().map(|i| (i.kind.as_str(), i.vis.as_str())).collect();
        assert!(kinds.contains(&("impl", "priv")));
        assert!(kinds.contains(&("fn", "priv")), "private fn in inventory");
        assert!(kinds.contains(&("fn", "pub")), "pub(crate) counts as pub boundary");
        assert_eq!(m.signatures.len(), 2, "pub + pub(crate) fns are the fan-out surface");
        // private method inside impl is inventory knowledge
        assert!(m.inventory.iter().any(|i| i.kind == "fn" && i.name == "default"));
    }
}
