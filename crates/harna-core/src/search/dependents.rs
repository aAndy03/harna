//! Component `dependents`: the reverse-edge query.
//!
//! "Who breaks if I change this?" — answered *before* the change, straight
//! from the digest's internal import graph. The planning half of parity:
//! fan-out tells you what you owe after a change; dependents tells you what
//! to expect before you make it.

use anyhow::Context as _;
use serde::Serialize;

use crate::schema::store::Store;
use crate::search::digest::{self, Digest};

#[derive(Debug, Clone, Serialize)]
pub struct DependentsReport {
    /// The resolved digest module path running the query against.
    pub module: String,
    /// Modules whose imports reach this module (ring 1).
    pub direct: Vec<DependentEdge>,
    /// Importers of the direct dependents (ring 2) — expected knock-on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ring_two: Vec<DependentEdge>,
    /// Direct dependents grouped by the (layer, container) that hosts them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependentEdge {
    pub importer: String,
    /// The dep id that creates the edge (use statement or inline reference).
    pub via: String,
}

impl DependentsReport {
    /// Token-efficient rendering for agent consumption.
    pub fn render(&self) -> String {
        if self.direct.is_empty() {
            return format!("dependents: nothing imports `{}` directly", self.module);
        }
        let mut out = format!(
            "dependents of `{}`: {} direct, {} knock-on (ring 2)\n",
            self.module,
            self.direct.len(),
            self.ring_two.len()
        );
        for e in &self.direct {
            out.push_str(&format!("  ↳ {} (via {})\n", e.importer, e.via));
        }
        if !self.ring_two.is_empty() {
            out.push_str("ring 2 (importers of those):\n");
            for e in &self.ring_two {
                out.push_str(&format!("  · {} (via {})\n", e.importer, e.via));
            }
        }
        if !self.hosts.is_empty() {
            out.push_str(&format!(
                "hosted by: {} — a change here should respect those \
                 layers' rules and declared dependencies\n",
                self.hosts.join(", ")
            ));
        }
        out
    }
}

/// Compute who imports `query` (digest path, crate-relative stem, or a
/// known file stem).
pub fn build(store: &Store, query: &str) -> anyhow::Result<DependentsReport> {
    let digest = store
        .load_digest()
        .context("no code map — run `harna scan` first; dependents reads the map, not the files")?;
    let module = resolve_module(&digest, query)?;
    let root = store
        .manifest_path()
        .parent()
        .and_then(|p| p.parent())
        .context("store root not found")?
        .to_path_buf();
    let crate_names = digest::workspace_crate_names(&root);
    let target_stem = digest::crate_of(&module).1;

    let mut direct = Vec::new();
    let mut ring_two_ids: Vec<String> = Vec::new();
    for m in &digest.modules {
        if m.path == module {
            continue;
        }
        let importer_stem = digest::crate_of(&m.path).1;
        if let Some(via) = m
            .deps
            .iter()
            .find(|dep| digest::dep_points_at(&importer_stem, dep, &module, &crate_names))
        {
            direct.push(DependentEdge { importer: m.path.clone(), via: via.clone() });
            ring_two_ids.push(m.path.clone());
        }
    }

    // Ring 2: importers of the direct dependents (excluding the module itself
    // and ring-1 duplicates); one edge per importer — the first dep that
    // creates it.
    let mut ring_two = Vec::new();
    for m in &digest.modules {
        if m.path == module || !ring_two_ids.contains(&m.path) {
            continue;
        }
        let importer_stem = digest::crate_of(&m.path).1;
        if let Some(dep) = m.deps.iter().find(|dep| {
            ring_two_ids.iter().any(|rt| {
                *rt != m.path
                    && !direct.iter().any(|e| e.importer == m.path)
                    && digest::dep_points_at(&importer_stem, dep, rt, &crate_names)
            })
        }) {
            ring_two.push(DependentEdge { importer: m.path.clone(), via: dep.clone() });
        }
    }

    // Hosts: the manifest containers whose path prefixes the module.
    let manifest = store.load_manifest().unwrap_or_default();
    let mut hosts = Vec::new();
    for l in &manifest.layers {
        for c in &l.containers {
            if !c.path.is_empty() {
                if module.starts_with(&format!("{}/", c.path)) {
                    hosts.push(format!("layer {}, container {}", l.name, c.name));
                }
            }
        }
    }

    Ok(DependentsReport { module, direct, ring_two, hosts })
}

/// Digest module identity from a query: exact path, crate-relative stem,
/// or file stem.
fn resolve_module<'d>(digest: &'d Digest, query: &str) -> anyhow::Result<String> {
    let q = query.replace('\\', "/").trim_matches('/').to_string();
    if let Some(m) = digest.modules.iter().find(|m| m.path == q) {
        return Ok(m.path.clone());
    }
    // stem match (e.g. "harna-core/src/schema/store" or "schema/store")
    if let Some(m) = digest
        .modules
        .iter()
        .find(|m| digest::crate_of(&m.path).1 == q || m.path.ends_with(&format!("/{q}")))
    {
        return Ok(m.path.clone());
    }
    // loose stem (file name)
    if let Some(m) = digest.modules.iter().find(|m| {
        m.path
            .rsplit('/')
            .next()
            .map(|f| f.trim_end_matches(".rs"))
            == Some(q.as_str())
    }) {
        return Ok(m.path.clone());
    }
    anyhow::bail!(
        "no digest module matches `{query}` — check `harna diff`/`harna status` \
         and re-run `harna scan` if the file is new"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::digest::Module;

    fn module(path: &str, sigs: &[&str], deps: &[&str]) -> Module {
        Module {
            path: path.into(),
            lang: "rust".into(),
            signatures: sigs.iter().map(|s| s.to_string()).collect(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            body_hash: "x".into(),
            inventory: vec![],
            mods: vec![],
        }
    }

    fn store_with() -> Store {
        let tmp = std::env::temp_dir().join(format!("harna-dep-{}", std::process::id()));
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let digest = Digest {
            generated_at: "now".into(),
            modules: vec![
                // cross-crate importer (workspace crate x)
                module("crates/x/src/api.rs", &[], &["x::core::store::load"]),
                // same-crate importer via super::
                module(
                    "crates/x/src/core/wrapper.rs",
                    &[],
                    &["super::store::compute"],
                ),
                // unrelated module: no edge to the store
                module("crates/x/src/core/logger.rs", &["pub fn log()"], &[]),
                module(
                    "crates/x/src/core/store.rs",
                    &["pub fn load()", "pub fn compute()"],
                    &[],
                ),
            ],
        };
        store.save_digest(&digest).unwrap();

        // Root Cargo.toml so workspace crate "x" resolves.
        std::fs::write(
            store.manifest_path().parent().unwrap().parent().unwrap().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        let tdir = "crates/x".to_string();
        let _ = tdir;
        let cargo = store
            .manifest_path()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("crates/x/Cargo.toml");
        std::fs::create_dir_all(cargo.parent().unwrap()).unwrap();
        std::fs::write(cargo, "[package]\nname = \"x\"\nversion = \"0.1.0\"\n").unwrap();
        store
    }

    #[test]
    fn dependents_finds_direct_and_rings() -> anyhow::Result<()> {
        let store = store_with();
        let r = build(&store, "crates/x/src/core/store.rs")?;
        assert_eq!(r.module, "crates/x/src/core/store.rs");
        let importers: Vec<&str> = r.direct.iter().map(|e| e.importer.as_str()).collect();
        assert!(importers.contains(&"crates/x/src/api.rs"), "cross-crate edge");
        assert!(
            importers.contains(&"crates/x/src/core/wrapper.rs"),
            "super:: edge resolves into the ring"
        );
        assert!(!importers.contains(&"crates/x/src/core/logger.rs"), "no false adjacency");
        // logger is not an importer of anything in ring 1, no ring-2 for it.
        assert!(r.ring_two.iter().all(|e| e.importer != "crates/x/src/core/logger.rs"));
        assert!(r.render().contains("crates/x/src/api.rs"));
        Ok(())
    }

    #[test]
    fn dependents_accepts_stem_queries_and_rejects_unknown() -> anyhow::Result<()> {
        let store = store_with();
        let r = build(&store, "store")?;
        assert_eq!(r.module, "crates/x/src/core/store.rs");
        assert!(build(&store, "does-not-exist.rs").is_err());
        assert!(build(&Store::open(&std::env::temp_dir().join("harna-dep-empty")).unwrap(), "x").is_err());
        Ok(())
    }
}
