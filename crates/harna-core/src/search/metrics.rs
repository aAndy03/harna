//! Component `metrics`: fan-in/fan-out and boundary pressure (0.3.12).
//!
//! The import graph as numbers: every digest module's incoming/outgoing
//! edges (resolved with the one shared host machinery — `resolve_dep` +
//! `hosted_module`, the same read structure/budgets/layering use), each
//! container's internal vs crossing edges, and the coupling outliers.
//! Boundary pressure is the share of homed edges that cross a container
//! WITHOUT the manifest declaring it (0.3.44 deno study, A10): an import
//! into a layer the importer's `depends_on` names is the architecture
//! working as drawn, not pressure on it. Sibling containers of one layer
//! and undeclared layer reaches are what a high-pressure map is made of:
//! a layering that exists on paper only.
//!
//! Edges are importer→imported pairs, deduplicated (five deps into one
//! module are one edge, not five); self-imports never form edges. Fan
//! counts run over all resolved edges; pressure only over edges whose
//! both ends have a manifest home (unhomed modules are adoption debt,
//! not boundary debt — the gateway check's ungoverned carve-out says so).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::schema::model::manifest::Manifest;
use crate::schema::store::Store;
use crate::search::digest::{self, crate_name_dirs, module_homes, resolve_dep, Digest};

/// One module's coupling: incoming + outgoing resolved edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleMetric {
    pub path: String,
    pub fan_in: usize,
    pub fan_out: usize,
    pub coupling: usize,
}

/// One container's edge ledger: internal edges plus the crossing halves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerMetric {
    pub container: String,
    pub layer: String,
    pub modules: usize,
    pub internal_edges: usize,
    pub inbound: usize,
    pub outbound: usize,
    /// Crossing edges as a share of all the container's edges (floored;
    /// 0 when the container carries no edges at all).
    pub pressure_pct: u8,
}

/// The whole map measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsReport {
    pub modules: Vec<ModuleMetric>,
    pub containers: Vec<ContainerMetric>,
    pub total_edges: usize,
    pub cross_container_edges: usize,
    pub cross_layer_edges: usize,
    /// Crossings the manifest does not declare (sibling containers, or a
    /// layer outside the importer's `depends_on`).
    #[serde(default)]
    pub undeclared_edges: usize,
    /// Undeclared crossings as a share of all homed edges (floored; 0
    /// with none).
    pub boundary_pressure_pct: u8,
    /// Modules coupled past the outlier threshold, busiest first.
    pub outliers: Vec<String>,
    /// Modules the manifest does not reach (no pressure signal from them).
    pub unhomed: usize,
}

/// Smallest coupling that ever flags: below this a module is ordinary no
/// matter what the statistics say (tiny repos must not manufacture
/// outliers out of three-import utilities).
pub const OUTLIER_FLOOR: usize = 8;

impl MetricsReport {
    /// Token-efficient rendering for humans and agents.
    pub fn render(&self) -> String {
        let mut out = format!(
            "metrics: {} modules, {} edges ({} cross-container, {} cross-layer, {} undeclared) · boundary pressure {}%\n",
            self.modules.len(),
            self.total_edges,
            self.cross_container_edges,
            self.cross_layer_edges,
            self.undeclared_edges,
            self.boundary_pressure_pct,
        );
        if self.outliers.is_empty() {
            out.push_str("coupling outliers: none\n");
        } else {
            out.push_str(&format!(
                "coupling outliers: {}\n",
                self.outliers.join(", ")
            ));
        }
        let mut containers: Vec<&ContainerMetric> = self.containers.iter().collect();
        containers.sort_by(|a, b| {
            b.pressure_pct
                .cmp(&a.pressure_pct)
                .then(a.container.cmp(&b.container))
        });
        for c in containers.iter().take(10) {
            out.push_str(&format!(
                "  {} ({}): {} modules, {} internal, +{}/-{} crossing, pressure {}%\n",
                c.container,
                c.layer,
                c.modules,
                c.internal_edges,
                c.inbound,
                c.outbound,
                c.pressure_pct
            ));
        }
        if containers.len() > 10 {
            out.push_str(&format!(
                "  …and {} more containers\n",
                containers.len() - 10
            ));
        }
        if self.unhomed > 0 {
            out.push_str(&format!(
                "  note: {} module(s) sit outside every container (no pressure signal)\n",
                self.unhomed
            ));
        }
        out
    }
}

/// Measure the committed map behind this store.
///
/// Bounded (0.3.16): shards stream one container at a time and only the
/// graph projection (path + deps) stays resident — signatures and
/// inventory, the byte bulk, never materialize. The resident set is the
/// import graph itself, which is what fan/pressure measurement is *of*;
/// edge resolution keeps the one shared machinery (`resolve_dep` +
/// `hosted_module`) via `build`, so bounded reads change the parse unit,
/// not the semantics.
pub fn build_store(store: &Store) -> anyhow::Result<MetricsReport> {
    let mut modules: Vec<digest::Module> = Vec::new();
    let found = store.for_each_shard(|_, shard| {
        for m in &shard.modules {
            modules.push(digest::Module {
                reexports: Vec::new(),
                crate_dir: None,
                src_root: None,
                path: m.path.clone(),
                lang: m.lang.clone(),
                signatures: Vec::new(),
                deps: m.deps.clone(),
                body_hash: String::new(),
                inventory: Vec::new(),
                mods: Vec::new(),
            });
        }
        true
    })?;
    if !found {
        anyhow::bail!("no code map — run `harna scan` first; metrics reads the map, not the files");
    }
    // No manifest yet is an honest "nothing homed"; a manifest that exists
    // but will not load would report every module unhomed and 0% pressure
    // — a fabricated reading, so it errors instead.
    let manifest = if store.manifest_path().exists() {
        store.load_manifest()?
    } else {
        Manifest::default()
    };
    let graph = Digest {
        generated_at: String::new(),
        extractor_version: 0,
        modules,
    };
    Ok(build(&store.repo_root(), &manifest, &graph))
}

/// Measure one manifest + digest pair.
pub fn build(root: &Path, manifest: &Manifest, digest: &Digest) -> MetricsReport {
    let edges = resolved_edges(root, digest);
    let homes = module_homes(manifest, digest);

    let mut fan_in: BTreeMap<&str, usize> = BTreeMap::new();
    let mut fan_out: BTreeMap<&str, usize> = BTreeMap::new();
    for m in &digest.modules {
        fan_in.entry(m.path.as_str()).or_insert(0);
        fan_out.entry(m.path.as_str()).or_insert(0);
    }
    for (from, to) in &edges {
        *fan_out.entry(from).or_insert(0) += 1;
        *fan_in.entry(to).or_insert(0) += 1;
    }
    let modules: Vec<ModuleMetric> = digest
        .modules
        .iter()
        .map(|m| {
            let (i, o) = (fan_in[m.path.as_str()], fan_out[m.path.as_str()]);
            ModuleMetric {
                path: m.path.clone(),
                fan_in: i,
                fan_out: o,
                coupling: i + o,
            }
        })
        .collect();

    // Pressure runs over homed pairs only; fan counts already ran above.
    let mut cross_container = 0usize;
    let mut cross_layer = 0usize;
    let mut undeclared = 0usize;
    let mut homed_edges = 0usize;
    let depends: BTreeMap<&str, Vec<&str>> = manifest
        .layers
        .iter()
        .map(|l| {
            (
                l.name.as_str(),
                l.depends_on.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    // (container path, layer) -> (internal, inbound, outbound)
    let mut ledger: BTreeMap<(&str, &str), (usize, usize, usize)> = BTreeMap::new();
    // (container path, layer) -> homed module count
    let mut homed_modules: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for home in homes.values() {
        *homed_modules
            .entry((home.container_path, home.layer))
            .or_insert(0) += 1;
    }
    for (from, to) in &edges {
        let (Some(f), Some(t)) = (homes.get(from), homes.get(to)) else {
            continue;
        };
        homed_edges += 1;
        let f_key = (f.container_path, f.layer);
        let t_key = (t.container_path, t.layer);
        ledger.entry(f_key).or_insert((0, 0, 0));
        ledger.entry(t_key).or_insert((0, 0, 0));
        if f_key == t_key {
            ledger.entry(f_key).or_insert((0, 0, 0)).0 += 1;
            continue;
        }
        if f.container_path != t.container_path {
            cross_container += 1;
        }
        if f.layer != t.layer {
            cross_layer += 1;
        }
        let declared =
            f.layer != t.layer && depends.get(f.layer).is_some_and(|d| d.contains(&t.layer));
        if f.container_path != t.container_path && !declared {
            undeclared += 1;
        }
        ledger.entry(f_key).or_insert((0, 0, 0)).2 += 1; // outbound
        ledger.entry(t_key).or_insert((0, 0, 0)).1 += 1; // inbound
    }

    let mut containers: Vec<ContainerMetric> = manifest
        .layers
        .iter()
        .flat_map(|l| {
            l.containers.iter().map(|c| {
                let key = (c.path.as_str(), l.name.as_str());
                let (internal, inbound, outbound) = ledger.get(&key).copied().unwrap_or((0, 0, 0));
                let cross = inbound + outbound;
                let total = internal + cross;
                ContainerMetric {
                    container: c.name.clone(),
                    layer: l.name.clone(),
                    modules: homed_modules.get(&key).copied().unwrap_or(0),
                    internal_edges: internal,
                    inbound,
                    outbound,
                    pressure_pct: (cross * 100)
                        .checked_div(total)
                        .map_or(0, |p| p.min(100) as u8),
                }
            })
        })
        .collect();
    containers.sort_by(|a, b| a.layer.cmp(&b.layer).then(a.container.cmp(&b.container)));

    let boundary_pressure_pct = (undeclared * 100)
        .checked_div(homed_edges)
        .map_or(0, |p| p.min(100) as u8);
    let unhomed = digest.modules.len() - homes.len().min(digest.modules.len());
    let outliers = outliers(&modules);

    MetricsReport {
        modules,
        containers,
        total_edges: edges.len(),
        cross_container_edges: cross_container,
        cross_layer_edges: cross_layer,
        undeclared_edges: undeclared,
        boundary_pressure_pct,
        outliers,
        unhomed,
    }
}

/// Importer→imported pairs over the digest, deduplicated, self-imports
/// excluded — the same resolution every graph consumer shares (0.3.19:
/// the graph export renders these same pairs; one resolution, no second
/// opinion).
pub fn resolved_edges<'d>(root: &Path, digest: &'d Digest) -> BTreeSet<(&'d str, &'d str)> {
    let crate_names = digest::workspace_crate_names(root);
    let dirs: BTreeSet<String> = digest
        .modules
        .iter()
        .filter_map(|m| m.crate_of().0)
        .filter(|dir| dir != ".")
        .collect();
    let name_dirs = crate_name_dirs(root, &dirs.into_iter().collect::<Vec<_>>());
    let hosts = crate::search::digest::HostIndex::new(digest);
    let mut edges = BTreeSet::new();
    for m in &digest.modules {
        let (importer_dir, importer_stem) = m.crate_of();
        for dep in &m.deps {
            let Some(target) = resolve_dep(dep, importer_stem.as_str(), &crate_names) else {
                continue;
            };
            let Some(hosted) = hosts.hosted(&name_dirs, importer_dir.as_deref(), &target) else {
                continue;
            };
            if hosted.path != m.path {
                edges.insert((m.path.as_str(), hosted.path.as_str()));
            }
        }
    }
    edges
}

/// Modules coupled past `max(OUTLIER_FLOOR, ceil(mean + 2σ))`, busiest
/// first. Statistics over the whole map; the floor keeps small repos
/// honest (no manufactured outliers).
fn outliers(modules: &[ModuleMetric]) -> Vec<String> {
    if modules.is_empty() {
        return Vec::new();
    }
    let n = modules.len() as f64;
    let mean = modules.iter().map(|m| m.coupling as f64).sum::<f64>() / n;
    let var = modules
        .iter()
        .map(|m| (m.coupling as f64 - mean).powi(2))
        .sum::<f64>()
        / n;
    let threshold = (OUTLIER_FLOOR as f64).max(mean + 2.0 * var.sqrt()).ceil() as usize;
    let mut flagged: Vec<&ModuleMetric> =
        modules.iter().filter(|m| m.coupling > threshold).collect();
    flagged.sort_by(|a, b| b.coupling.cmp(&a.coupling).then(a.path.cmp(&b.path)));
    flagged.into_iter().map(|m| m.path.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};

    fn module(path: &str, deps: &[&str]) -> digest::Module {
        digest::Module {
            reexports: Vec::new(),
            crate_dir: None,
            src_root: None,
            path: path.into(),
            lang: "rust".into(),
            signatures: vec![],
            deps: deps.iter().map(|s| s.to_string()).collect(),
            body_hash: "x".into(),
            inventory: vec![],
            mods: vec![],
        }
    }

    fn container(name: &str, path: &str) -> Container {
        Container {
            escapes: Vec::new(),
            owns: Vec::new(),
            name: name.into(),
            path: path.into(),
            description: String::new(),
            sequence: vec![],
            provides: vec![],
            role: None,
            gateway: None,
            connects: vec![],
            components: vec![],
        }
    }

    fn manifest(layers: Vec<(&str, Vec<Container>)>) -> Manifest {
        Manifest {
            schema_version: 1,
            layers: layers
                .into_iter()
                .map(|(name, containers)| Layer {
                    name: name.into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    role: None,
                    containers,
                })
                .collect(),
        }
    }

    fn digest_of(modules: Vec<digest::Module>) -> Digest {
        Digest {
            generated_at: "now".into(),
            extractor_version: digest::EXTRACTOR_VERSION,
            modules,
        }
    }

    fn root() -> std::path::PathBuf {
        // No Cargo.tomls needed: same-crate `crate::` deps never consult
        // the package index (external targets stay inside the importer's
        // crate dir by construction).
        std::env::temp_dir().join(format!(
            "harna-metrics-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn fan_counts_dedupe_and_skip_self_imports() {
        let m = manifest(vec![("l", vec![container("c", "crates/t/src")])]);
        let d = digest_of(vec![
            module(
                "crates/t/src/a.rs",
                &["crate::b::f", "crate::b::g", "crate::a::f"],
            ),
            module("crates/t/src/b.rs", &[]),
        ]);
        let r = build(&root(), &m, &d);
        assert_eq!(
            r.total_edges, 1,
            "two deps into b are one edge; self skipped"
        );
        let a = r.modules.iter().find(|x| x.path.ends_with("a.rs")).unwrap();
        let b = r.modules.iter().find(|x| x.path.ends_with("b.rs")).unwrap();
        assert_eq!((a.fan_out, a.fan_in), (1, 0));
        assert_eq!((b.fan_in, b.fan_out), (1, 0));
        assert_eq!(r.boundary_pressure_pct, 0, "same container: no pressure");
    }

    #[test]
    fn pressure_counts_homed_pairs_only() {
        let m = manifest(vec![
            ("up", vec![container("one", "crates/t/src/one")]),
            ("down", vec![container("two", "crates/t/src/two")]),
        ]);
        let d = digest_of(vec![
            module("crates/t/src/one/a.rs", &["crate::two::b::f"]),
            module("crates/t/src/two/b.rs", &[]),
            // Unmapped file: real edge for fan counts, invisible to pressure.
            module("crates/t/src/stray.rs", &["crate::two::b::f"]),
        ]);
        let r = build(&root(), &m, &d);
        assert_eq!(r.total_edges, 2);
        assert_eq!(r.cross_container_edges, 1);
        assert_eq!(r.cross_layer_edges, 1);
        // Pressure runs over homed edges only: the only homed edge crosses,
        // so pressure is total — the stray edge is unhomed adoption debt,
        // not diluting boundary debt (its count surfaces as `unhomed`).
        assert_eq!(r.boundary_pressure_pct, 100);
        assert_eq!(r.unhomed, 1);
        let one = r.containers.iter().find(|c| c.container == "one").unwrap();
        assert_eq!((one.outbound, one.inbound, one.internal_edges), (1, 0, 0));
        assert_eq!(one.pressure_pct, 100);
    }

    /// deno study A10 (0.3.45): harna graded itself 65 (D) because its
    /// interface crates import core exactly as `depends_on` declares. A
    /// declared reach is the architecture working; only undeclared
    /// crossings (sibling containers, unlisted layers) are pressure.
    #[test]
    fn declared_layer_reaches_are_not_pressure() {
        let mut m = manifest(vec![
            ("up", vec![container("one", "crates/t/src/one")]),
            (
                "down",
                vec![
                    container("two", "crates/t/src/two"),
                    container("three", "crates/t/src/three"),
                ],
            ),
        ]);
        m.layers[0].depends_on = vec!["down".into()];
        let d = digest_of(vec![
            module("crates/t/src/one/a.rs", &["crate::two::b::f"]),
            module("crates/t/src/two/b.rs", &["crate::three::c::f"]),
            module("crates/t/src/three/c.rs", &[]),
        ]);
        let r = build(&root(), &m, &d);
        assert_eq!((r.cross_container_edges, r.cross_layer_edges), (2, 1));
        assert_eq!(r.undeclared_edges, 1, "only the sibling reach two -> three");
        assert_eq!(r.boundary_pressure_pct, 50);
        assert!(r.render().contains("1 undeclared"));
    }

    #[test]
    fn floor_keeps_small_repos_honest_and_empty_maps_zero() {
        let m = manifest(vec![("l", vec![container("c", "crates/t/src")])]);
        // A star: hub with 6 spokes — busy, but below the floor.
        let mut mods = vec![module("crates/t/src/hub.rs", &[])];
        for i in 0..6 {
            mods.push(module(
                &format!("crates/t/src/s{i}.rs"),
                &[&format!("crate::hub::f{i}")],
            ));
        }
        let r = build(&root(), &m, &digest_of(mods));
        assert!(r.outliers.is_empty(), "coupling 6 never flags (floor 8)");
        let empty = build(&root(), &m, &digest_of(vec![]));
        assert_eq!((empty.total_edges, empty.boundary_pressure_pct), (0, 0));
        assert!(empty.outliers.is_empty());
    }

    #[test]
    fn statistics_flag_the_true_hub() {
        // 12 leaves each importing the hub AND one shared util: hub
        // coupling 12 clears mean+2σ; util coupling 12 ties it — both flag
        // only when past the statistics, ordered busiest-first.
        let m = manifest(vec![("l", vec![container("c", "crates/t/src")])]);
        let mut mods = vec![
            module("crates/t/src/hub.rs", &[]),
            module("crates/t/src/util.rs", &[]),
        ];
        for i in 0..12 {
            mods.push(module(
                &format!("crates/t/src/l{i}.rs"),
                &[&format!("crate::hub::h{i}"), &format!("crate::util::u{i}")],
            ));
        }
        let r = build(&root(), &m, &digest_of(mods));
        // couplings: hub 12, util 12, leaves 2 each. mean = (24+24)/14 ≈ 3.4,
        // σ ≈ 3.7 → threshold ceil(3.4+7.4) = 11 → both flag at 12.
        assert_eq!(r.outliers.len(), 2, "hub + util, was: {:?}", r.outliers);
        assert!(r.outliers[0].ends_with("hub.rs"), "ties break by path");
    }

    #[test]
    fn unhosted_deps_never_form_edges() {
        let m = manifest(vec![("l", vec![container("c", "crates/t/src")])]);
        let d = digest_of(vec![module("crates/t/src/a.rs", &["crate::ghost::f"])]);
        let r = build(&root(), &m, &d);
        assert_eq!(r.total_edges, 0);
        assert_eq!(r.modules[0].coupling, 0);
    }

    #[test]
    fn build_store_streams_shards_and_measures_the_same_map() {
        // The bounded path (0.3.16): build_store reads the sharded layout
        // (save_digest writes shards) and keeps only the graph projection —
        // the numbers must equal what the whole-map era produced.
        let tmp = std::env::temp_dir().join(format!(
            "harna-metrics-store-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let m = manifest(vec![("l", vec![container("c", "crates/t/src")])]);
        store.save_manifest(&m).unwrap();
        let d = digest_of(vec![
            module("crates/t/src/a.rs", &["crate::b::f", "crate::b::g"]),
            module("crates/t/src/b.rs", &[]),
        ]);
        store.save_digest(&m, &d).unwrap();
        let r = build_store(&store).unwrap();
        assert_eq!(r.total_edges, 1, "two deps into b are one edge");
        assert_eq!(r.modules.len(), 2);
        assert_eq!(r.containers.len(), 1);
        assert_eq!(r.containers[0].modules, 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_store_refuses_mapless_repos_with_the_scan_hint() {
        let tmp = std::env::temp_dir().join(format!(
            "harna-metrics-empty-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let err = build_store(&store).unwrap_err().to_string();
        assert!(err.contains("run `harna scan` first"), "was: {err}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_broken_manifest_errors_instead_of_reading_as_unhomed() {
        let tmp = std::env::temp_dir().join(format!(
            "harna-metrics-broken-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let m = manifest(vec![("l", vec![container("c", "crates/t/src")])]);
        store
            .save_digest(&m, &digest_of(vec![module("crates/t/src/a.rs", &[])]))
            .unwrap();
        std::fs::write(store.manifest_path(), "schema_version = 1\n[[layers]\n").unwrap();
        let err = format!("{:#}", build_store(&store).unwrap_err());
        assert!(
            err.contains("manifest.toml"),
            "names the broken file: {err}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
