//! Workspace crates and dependency resolution: which crate a `use`
//! names, and which map module a dep id points at — the single resolver
//! fan-out, layer boundaries and dependents all share.

use super::*;

/// Names of crates declared in the workspace — `use <name>::`
/// statements in member crates are internal deps, not external ones.
/// Public so the parity ledger can resolve cross-crate import edges.
pub fn workspace_crate_names(root: &Path) -> Vec<String> {
    // 0.3.44: the Cargo-driven index — packages, lib names and `package =`
    // renames, implicit path members included.
    let names = super::crates::CrateIndex::load(root).names();
    if !names.is_empty() {
        return names;
    }
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
pub(super) fn collect_member_crate_names(root: &Path, member: &str, out: &mut Vec<String>) {
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

pub(super) fn push_crate_name(cargo: &Path, out: &mut Vec<String>) {
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

pub fn resolve_dep(dep: &str, importer_stem: &str, crate_names: &[String]) -> Option<DepTarget> {
    let importer = importer_stem.trim_end_matches("/mod");
    if let Some(rest) = dep.strip_prefix("crate::") {
        return Some(DepTarget {
            external: None,
            stem: stem_of_path(rest),
        });
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
        return Some(DepTarget {
            external: None,
            stem,
        });
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
        return Some(DepTarget {
            external: None,
            stem,
        });
    }
    let head = dep.split("::").next().unwrap_or("");
    if let Some(pkg) = crate_names.iter().find(|n| crate_name_eq(n, head)) {
        let rest = dep.split_once("::").map(|(_, r)| r).unwrap_or("");
        // The matched WORKSPACE PACKAGE name rides in the target, not the
        // import-site spelling: crates import `a-b` as `a_b`, and the
        // host lookup (`crate_name_dirs`) keys on package names. Before
        // 0.3.19 this carried `head` and cross-crate edges silently
        // dropped out of every consumer sharing this resolution.
        return Some(DepTarget {
            external: Some(pkg.clone()),
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
    // 0.3.44: every import name (package, lib name, rename) maps to its
    // crate dir through the Cargo-driven index.
    let index = super::crates::CrateIndex::load(root);
    for (name, pkg) in &index.import_names {
        if let Some(dir) = index.dir_of(pkg) {
            if dirs.iter().any(|d| d == dir) {
                map.insert(name.clone(), dir.to_string());
                map.insert(pkg.clone(), dir.to_string());
            }
        }
    }
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

/// Hosted-module lookups in O(stem depth) instead of a scan of the whole
/// map per dependency (0.3.46: `hosted_module` over every module made
/// codex's no-cycles + surface checks take 100s of a 168s validate). Built
/// once per check; answers exactly what `hosted_module` answers.
pub struct HostIndex<'a> {
    by_stem: std::collections::HashMap<(String, String), &'a Module>,
}

impl<'a> HostIndex<'a> {
    pub fn new(digest: &'a Digest) -> HostIndex<'a> {
        let mut by_stem = std::collections::HashMap::new();
        for m in &digest.modules {
            let (dir, stem) = m.crate_of();
            if let Some(dir) = dir {
                by_stem.entry((dir, stem)).or_insert(m);
            }
        }
        HostIndex { by_stem }
    }

    pub fn hosted(
        &self,
        name_dirs: &std::collections::BTreeMap<String, String>,
        importer_dir: Option<&str>,
        target: &DepTarget,
    ) -> Option<&'a Module> {
        let importer_dir = importer_dir.unwrap_or(".");
        let target_dir = match target.external.as_deref() {
            Some(name) => name_dirs.get(name)?.as_str(),
            None => importer_dir,
        };
        let get = |stem: &str| {
            self.by_stem
                .get(&(target_dir.to_string(), stem.to_string()))
                .copied()
        };
        // longest hosting stem first: the stem, then each shorter prefix
        let mut stem = target.stem.as_str();
        loop {
            if let Some(m) = get(stem) {
                return Some(m);
            }
            match stem.rsplit_once('/') {
                Some((head, _)) => stem = head,
                None => break,
            }
        }
        let leaf = target_dir
            .rsplit('/')
            .next()
            .unwrap_or(target_dir)
            .replace('-', "_");
        get("lib").or_else(|| get("main")).or_else(|| get(&leaf))
    }
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
    HostIndex::new(digest).hosted(name_dirs, importer_dir, target)
}

/// A digest module's home: the container whose path prefixes the module
/// (longest container path wins when containers nest) and that
/// container's layer. Defined in memory (`schema::manifest`, 0.3.15) —
/// it joins two memory types — and re-exported here so every existing
/// consumer keeps its import path (search measures, checks gate, the
/// store shards, all sharing the one resolution).
pub use crate::schema::model::manifest::{module_homes, module_layers, ModuleHome};

pub(super) fn stem_of_path(s: &str) -> String {
    s.split("::")
        .filter(|x| !x.is_empty() && *x != "*")
        .collect::<Vec<_>>()
        .join("/")
}

/// Does internal dep `dep` of module `importer_stem` reach candidate module
/// `module_path`? The graph semantics shared by every consumer (parity
/// fan-out, layer boundaries, dependents queries): resolution from
/// `resolve_dep`, then stem-prefix matching against the candidate's stem.
/// A non-Rust edge (0.6.0): `Some(Some(path))` for a resolved file,
/// `Some(None)` for a package or an unresolved specifier, `None` for a
/// Rust dep (the resolver's business).
pub fn file_edge(dep: &str) -> Option<Option<&str>> {
    if let Some(p) = dep
        .strip_prefix("file:")
        .or_else(|| dep.strip_prefix("bridge:"))
    {
        return Some(Some(p));
    }
    if dep.starts_with("npm:") || dep.starts_with("file?:") {
        return Some(None);
    }
    None
}

pub fn dep_points_at(
    importer_stem: &str,
    dep: &str,
    module_path: &str,
    crate_names: &[String],
) -> bool {
    // a file edge (0.6.0, TS) names its target exactly; packages and
    // unresolved specifiers reach no module
    if let Some(edge) = file_edge(dep) {
        return edge.is_some_and(|p| p == module_path);
    }
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
    stems_meet(&target.stem, &stem)
}

/// The dep reaches the candidate when their module stems meet exactly, or
/// when one is inside the other's module boundary: the dep deeper
/// (candidate file, symbols suffixing it), or the candidate deeper (the dep
/// names an ancestor module that disseminates it).
fn stems_meet(dep_stem: &str, candidate_stem: &str) -> bool {
    dep_stem == candidate_stem
        || dep_stem.starts_with(&format!("{candidate_stem}/"))
        || candidate_stem.starts_with(&format!("{dep_stem}/"))
}

/// Crate-aware reach (0.3.44): like `dep_points_at`, but the target must
/// sit in the crate the dep names. Stem-only matching tied every
/// `<crate>::ops` import to every `ops.rs` of every crate — harmless in a
/// `crates/*/src` workspace of distinct stems, wrong across deno's ~90
/// crates that each carry `lib.rs`/`ops.rs`. Unknown crates (an older map,
/// a name the index lacks) keep the stem-only reading.
pub fn dep_reaches(
    importer: &Module,
    dep: &str,
    candidate: &Module,
    crate_names: &[String],
    name_dirs: &std::collections::BTreeMap<String, String>,
) -> bool {
    if let Some(edge) = file_edge(dep) {
        return edge.is_some_and(|p| p == candidate.path);
    }
    let (importer_dir, importer_stem) = importer.crate_of();
    let (candidate_dir, candidate_stem) = candidate.crate_of();
    if candidate_stem.is_empty() {
        return false;
    }
    let Some(target) = resolve_dep(dep, &importer_stem, crate_names) else {
        return false;
    };
    if target.stem.is_empty() {
        return false;
    }
    let target_dir = match target.external.as_deref() {
        Some(name) => name_dirs.get(name).cloned(),
        None => importer_dir,
    };
    if let (Some(t), Some(c)) = (target_dir.as_deref(), candidate_dir.as_deref()) {
        if t != c {
            return false;
        }
    }
    stems_meet(&target.stem, &candidate_stem)
}
