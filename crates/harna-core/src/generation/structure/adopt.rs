//! Component `adopt` (0.3.20): draft a manifest from the existing tree.
//!
//! The proposal is data, never a verdict: a scan of the tree grouped by
//! its own crate layout — one layer per packet of source (named after the
//! package, cross-crate import direction proposed as `depends_on` via
//! `metrics::resolved_edges`, the one shared resolution), one container
//! per crate `src`, one primitive component per module with its size in
//! the summary (`draft:` prefixed — a marker to replace with prose, not
//! prose pretending to be a decision). Adoption stays an edit away:
//! `harna adopt` prints the draft to stdout, the adopter prunes, renames
//! and only then commits it into `.harna/manifest.toml`.
//!
//! Pure over the tree: `build_incremental` scans, nothing touches
//! `.harna/` — a draft is a proposal about an unadopted repo, printed and
//! gone. File sizes are counted on a second read here (one-time, report
//! only) so the digest's data format stays untouched.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context as _;

use crate::schema::model::manifest::{Component, ComponentKind, Container, Layer, Manifest};
use crate::search::digest;
use crate::search::metrics;

/// The draft: everything needed to render it as manifest TOML.
#[derive(Debug, Clone)]
pub struct Draft {
    pub manifest: Manifest,
    /// Human summary — counts the adopter starts from.
    pub module_count: usize,
    pub layer_count: usize,
    pub container_count: usize,
    pub cross_crate_edges: usize,
    /// Budget rules proposed from the repo's own distribution (0.3.44).
    pub calibration: Vec<calibrate::Calibration>,
}

/// Sanitize a proposed identifier: lowercase, dashes/spaces become
/// underscores, anything outside `[a-z0-9_]` dropped. Deterministic.
fn ident(raw: &str) -> String {
    let s: String = raw
        .to_lowercase()
        .chars()
        .map(|c| match c {
            '-' | ' ' => '_',
            c if c.is_ascii_alphanumeric() || c == '_' => c,
            _ => '_',
        })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() {
        "unnamed".to_string()
    } else {
        s
    }
}

/// The package name a crate dir carries, read from its Cargo.toml (honest
/// toml read; falls back to the dir's own stem, then "adopted").
fn package_name(root: &Path, dir: &str) -> String {
    let base = if dir == "." {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    if let Ok(src) = std::fs::read_to_string(base.join("Cargo.toml")) {
        if let Ok(v) = toml::from_str::<toml::Value>(&src) {
            if let Some(n) = v
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
            {
                return n.to_string();
            }
        }
    }
    match dir {
        "." => "adopted".to_string(),
        d => d.rsplit(['/', '\\']).next().unwrap_or(d).to_string(),
    }
}

/// Component display name from a module's crate-relative stem, unique
/// within the container: the leaf first, the full stem on collision.
fn unique_name(stem: &str, used: &mut Vec<String>) -> String {
    let leaf = stem.rsplit('/').next().unwrap_or(stem);
    let mut candidate = ident(leaf);
    if used.contains(&candidate) {
        candidate = ident(stem);
    }
    if used.contains(&candidate) {
        let mut n = 2;
        loop {
            let numbered = format!("{candidate}_{n}");
            if !used.contains(&numbered) {
                candidate = numbered;
                break;
            }
            n += 1;
        }
    }
    used.push(candidate.clone());
    candidate
}

/// File sizes are counted on a second read here (one-time, report only)
/// so the digest's data format stays untouched.
fn count_lines(root: &Path, module_path: &str) -> usize {
    std::fs::read_to_string(root.join(module_path))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// Build the draft manifest for the tree at `root`. The scan is the map's
/// own machinery; the proposal below never re-decides what the scan meant.
pub fn draft(root: &Path, max_files: usize) -> anyhow::Result<Draft> {
    let (digest, _reused, _scanned) =
        digest::build_incremental(root, max_files, None, true).context("scanning the tree")?;
    if digest.modules.is_empty() {
        anyhow::bail!(
            "no rust modules found under {} — nothing to draft",
            root.display()
        );
    }

    // Crate dirs straight off the map (no manifest exists yet): every
    // distinct crate dir, root crate folded to ".". `None < Some` in
    // Option's order, so a plain sort puts the root crate first; module
    // paths arrive sorted, so dir order is deterministic.
    let mut dirs: Vec<Option<String>> = digest.modules.iter().map(|m| m.crate_of().0).collect();
    dirs.sort();
    dirs.dedup();

    let layer_names: BTreeMap<Option<String>, String> = dirs
        .iter()
        .map(|d| (d.clone(), package_name(root, d.as_deref().unwrap_or("."))))
        .collect();

    // Cross-crate import direction proposed as depends_on: layer A depends
    // on layer B when an edge crosses their crates (shared resolution).
    let edges = metrics::resolved_edges(root, &digest);
    let mut depends: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut cross_crate = 0usize;
    let index = crate::search::digest::crates::CrateIndex::load(root);
    for (from, to) in &edges {
        let f = index.crate_of(from).0;
        let t = index.crate_of(to).0;
        if f == t {
            continue;
        }
        cross_crate += 1;
        let f_name = layer_names.get(&f).cloned().unwrap_or_default();
        let t_name = layer_names.get(&t).cloned().unwrap_or_default();
        if f_name == t_name || t_name.is_empty() || f_name.is_empty() {
            continue;
        }
        let list = depends.entry(ident(&f_name)).or_default();
        let t_ident = ident(&t_name);
        if !list.contains(&t_ident) {
            list.push(t_ident);
        }
    }

    let mut layers = Vec::new();
    for dir in &dirs {
        let dir_path = dir.as_deref().unwrap_or(".");
        let layer_name = layer_names.get(dir).cloned().unwrap_or_default();
        // Container: one per crate, path = the crate's src root.
        let container_path = match dir {
            None => "src".to_string(),
            // 0.3.44: the crate's declared source root (`ext/fs` for a
            // root `lib.rs`), else the `src/` convention.
            Some(d) => index
                .crates
                .iter()
                .find(|c| &c.dir == d)
                .map(|c| c.src_root.clone())
                .unwrap_or_else(|| format!("{d}/src")),
        };
        let container_name = ident(
            dir_path
                .rsplit(['/', '\\'])
                .find(|seg| !seg.is_empty())
                .unwrap_or(dir_path),
        );
        let crate_modules: Vec<_> = digest
            .modules
            .iter()
            .filter(|m| m.crate_of().0 == *dir)
            .collect();
        let total_lines: usize = crate_modules
            .iter()
            .map(|m| count_lines(root, &m.path))
            .sum();
        let mut used = Vec::new();
        let components = crate_modules
            .iter()
            .map(|m| {
                let stem = m.crate_of().1;
                let lines = count_lines(root, &m.path);
                Component {
                    name: unique_name(&stem, &mut used),
                    kind: ComponentKind::Primitive,
                    path: m.path.clone(),
                    summary: format!("draft: {} lines", lines),
                    tests: Vec::new(),
                    provides: Vec::new(),
                    children: Vec::new(),
                }
            })
            .collect();
        layers.push(Layer {
            name: ident(&layer_name),
            description: format!(
                "drafted from {} — {} module(s), {} lines; a proposal to prune, not a verdict",
                container_path.trim_end_matches("/src"),
                crate_modules.len(),
                total_lines
            ),
            sequence: Vec::new(),
            depends_on: depends.remove(&ident(&layer_name)).unwrap_or_default(),
            role: None,
            containers: vec![Container {
                name: container_name,
                path: container_path,
                description: "drafted container — group its components as they deserve".to_string(),
                sequence: Vec::new(),
                provides: Vec::new(),
                role: None,
                gateway: None,
                connects: Vec::new(),
                components,
            }],
        });
    }

    let sized: Vec<(String, usize)> = digest
        .modules
        .iter()
        .map(|m| (m.path.clone(), count_lines(root, &m.path)))
        .collect();
    let calibration = calibrate::calibrate(&sized);
    let (ls, cs) = (
        layers.len(),
        layers.iter().map(|l| l.containers.len()).sum::<usize>(),
    );
    Ok(Draft {
        manifest: Manifest {
            schema_version: crate::SCHEMA_VERSION,
            layers,
        },
        module_count: digest.modules.len(),
        layer_count: ls,
        container_count: cs,
        cross_crate_edges: cross_crate,
        calibration,
    })
}

/// `harna adopt` for any repo (0.3.43): no manifest (or an empty one)
/// gets the fresh draft; an adopted repo gets the refresh. Returns
/// (summary line, printable body); nothing is written either way. A
/// manifest that does not parse fails loudly, never as "fresh start".
pub fn run(
    root: &Path,
    store: &crate::schema::store::Store,
    max_files: usize,
) -> anyhow::Result<(String, String)> {
    let existing = if store.manifest_path().is_file() {
        store.load_manifest()?
    } else {
        Manifest::default()
    };
    if !existing.is_empty() {
        let r = refresh::refresh(root, &existing, max_files)?;
        return Ok((r.summary(), r.render()));
    }
    let d = draft(root, max_files)?;
    let mut summary = format!(
        "draft: {} module(s) across {} layer(s)/{} container(s); {} cross-crate edge(s) proposed as depends_on",
        d.module_count, d.layer_count, d.container_count, d.cross_crate_edges
    );
    if !d.calibration.is_empty() {
        summary.push_str(
            "\ncalibrated budgets (this repo's p95 — run after `harna init`, tighten by choice):",
        );
        for c in &d.calibration {
            summary.push_str(&format!("\n  {}", c.command()));
        }
    }
    Ok((summary, render(&d)?))
}

pub mod calibrate;
pub mod refresh;

/// The draft rendered as manifest TOML — paste into `.harna/manifest.toml`
/// (after pruning). Byte-honest: the serializer's field order, not the
/// hand-style order, is fine for a starting draft.
pub fn render(draft: &Draft) -> anyhow::Result<String> {
    toml::to_string_pretty(&draft.manifest).context("serializing the draft manifest")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> std::path::PathBuf {
        // A two-crate workspace plus a stray: the shape adopt must propose
        // for. On-disk (Cargo.tomls give the package names; sources give
        // the scan something to see).
        let tmp = std::env::temp_dir().join(format!(
            "harna-adopt-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("crates/core/src/schema")).unwrap();
        std::fs::create_dir_all(tmp.join("crates/cli/src")).unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/core\", \"crates/cli\"]\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("crates/core/Cargo.toml"),
            "[package]\nname = \"harna-core\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("crates/core/src/schema/store.rs"),
            "pub struct Store;\nfn helper() {}\nfn helper2() {}\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("crates/core/src/schema/migrate.rs"),
            "use crate::schema::store::Store;\npub fn go() {}\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("crates/cli/Cargo.toml"),
            "[package]\nname = \"harna-cli\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("crates/cli/src/main.rs"),
            "use harna_core::schema::store::Store;\nfn main() {}\n",
        )
        .unwrap();
        tmp
    }

    #[test]
    fn draft_proposes_a_layer_per_crate_with_sizes_and_direction() {
        let tmp = fixture();
        let d = draft(&tmp, 10_000).unwrap();

        // Layer order is deterministic: module paths arrive sorted, so
        // crates/cli sorts before crates/core; the layer names carry the
        // package names as data.
        assert_eq!(
            d.manifest
                .layers
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>(),
            vec!["harna_cli", "harna_core"]
        );
        let core = d
            .manifest
            .layers
            .iter()
            .find(|l| l.name == "harna_core")
            .unwrap();
        assert_eq!(core.containers[0].path, "crates/core/src");
        assert_eq!(
            core.containers[0]
                .components
                .iter()
                .map(|c| c.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "crates/core/src/schema/migrate.rs",
                "crates/core/src/schema/store.rs"
            ]
        );
        // Sizes live in the summary, `draft:` marked to be replaced.
        assert_eq!(core.containers[0].components[1].summary, "draft: 3 lines");
        // Cross-crate direction proposed from the shared resolution:
        // cli -> core.
        let cli = d
            .manifest
            .layers
            .iter()
            .find(|l| l.name == "harna_cli")
            .unwrap();
        assert_eq!(cli.depends_on, vec!["harna_core".to_string()]);
        assert!(core.depends_on.is_empty());
        assert_eq!(d.cross_crate_edges, 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn draft_renders_loader_honest_toml() {
        let tmp = fixture();
        let d = draft(&tmp, 10_000).unwrap();
        let text = render(&d).unwrap();
        // The draft round-trips through the manifest loader byte-equal in
        // shape: a draft that fails to load would waste the adopter's
        // time (the whole point of drafting).
        let back: Manifest = toml::from_str(&text).unwrap();
        assert_eq!(back.layers.len(), d.manifest.layers.len());
        assert_eq!(back.schema_version, crate::SCHEMA_VERSION);
        assert!(text.contains("draft: 3 lines"));
        assert!(text.contains("name = \"harna_cli\""));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ident_sanitizes_and_names_stay_unique() {
        assert_eq!(ident("harna-cli"), "harna_cli");
        assert_eq!(ident("Didn't work!"), "didn_t_work");
        assert_eq!(ident("---"), "unnamed");

        let mut used = Vec::new();
        assert_eq!(unique_name("schema/store", &mut used), "store");
        assert_eq!(unique_name("schema2/store", &mut used), "schema2_store");
        assert_eq!(unique_name("schema3/store999", &mut used), "store999");
        // Full stem also taken -> numbered fallback, deterministic.
        used.clear();
        assert_eq!(unique_name("schema/store", &mut used), "store");
        assert_eq!(unique_name("q/store", &mut used), "q_store");
    }
}
