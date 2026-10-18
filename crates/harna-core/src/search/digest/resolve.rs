//! Workspace crates and dependency resolution: which crate a `use`
//! names, and which map module a dep id points at — the single resolver
//! fan-out, layer boundaries and dependents all share.

use super::*;

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
    if target.stem == stem
        || target.stem.starts_with(&candidate_border)
        || stem.starts_with(&dep_border)
    {
        return true;
    }
    false
}
