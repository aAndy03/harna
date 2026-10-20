//! Component `deny`: compile-time layering.
//!
//! Renders the manifest's layer topology into a `deny.toml` for cargo-deny:
//! every workspace crate found under containers becomes a ban rule whose
//! `wrappers` list is exactly the set of crates allowed to depend on it —
//! itself (same layer) plus crates whose layers are declared in the
//! depender's `depends_on`. A wrong cross-crate import stops being a review
//! comment and becomes a build error: `cargo deny check bans`.
//!
//! Module-level (intra-crate) layering stays in `validate`; cargo-deny sees
//! only the compile boundary, which is the crate.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Context as _;

use crate::schema::model::manifest::Manifest;

/// One ban rule: `crate` may only be depended upon by `wrappers`.
#[derive(Debug, Clone)]
pub struct DenyRule {
    pub crate_name: String,
    pub wrappers: Vec<String>,
    pub layers: Vec<String>,
    /// The deny entry's `reason`: the layers, or the gateway that owns an
    /// external crate.
    pub reason: String,
}

/// The full plan: rules keyed for deterministic rendering.
#[derive(Debug, Clone, Default)]
pub struct DenyPlan {
    pub rules: Vec<DenyRule>,
}

/// Compute the deny plan from the manifest: for each workspace crate, who may
/// depend on it.
pub fn plan(root: &Path, manifest: &Manifest) -> anyhow::Result<DenyPlan> {
    // crate package name -> set of home layers
    let index = crate::search::digest::crates::CrateIndex::load(root);
    let mut homes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for layer in &manifest.layers {
        for container in &layer.containers {
            let Some(crate_name) = crate_of(root, &index, &container.path)? else {
                continue;
            };
            let entry = homes.entry(crate_name).or_default();
            if !entry.contains(&layer.name) {
                entry.push(layer.name.clone());
            }
        }
    }

    // layer -> declared dependencies (one level; v1)
    let depends: BTreeMap<&str, Vec<&str>> = manifest
        .layers
        .iter()
        .map(|l| {
            (
                l.name.as_str(),
                l.depends_on.iter().map(|s| s.as_str()).collect(),
            )
        })
        .collect();

    let crate_names: Vec<&String> = homes.keys().collect();
    let mut rules = Vec::new();
    for (denied, denied_layers) in &homes {
        let mut wrappers = Vec::new();
        for (depender, depender_layers) in &homes {
            if depender == denied {
                continue;
            }
            let allowed = depender_layers.iter().any(|homer_layer| {
                denied_layers.contains(homer_layer)
                    || depends.get(homer_layer.as_str()).is_some_and(|deps| {
                        deps.iter().any(|d| denied_layers.contains(&d.to_string()))
                    })
            });
            if allowed {
                wrappers.push(depender.clone());
            }
        }
        wrappers.sort();
        let mut layers = denied_layers.clone();
        layers.sort();
        rules.push(DenyRule {
            crate_name: denied.clone(),
            wrappers,
            reason: format!("layer(s) {} only", layers.join(", ")),
            layers,
        });
    }
    let _ = crate_names;
    // External crates a gateway owns (0.3.48, R2): only the owner's crate
    // wraps them — codex's `reqwest` behind `codex-http-client`.
    for (name, owners) in manifest.crate_gateways() {
        let mut wrappers: Vec<String> = owners
            .iter()
            .filter_map(|o| crate_of(root, &index, o).ok().flatten())
            .collect();
        wrappers.sort();
        wrappers.dedup();
        rules.push(DenyRule {
            crate_name: name,
            reason: format!("gateway {} only", owners.join(", ")),
            wrappers,
            layers: Vec::new(),
        });
    }
    Ok(DenyPlan { rules })
}

/// Which workspace crate does a container path live in?
/// `crates/x/src/a` -> package name of `crates/x`; `src/...` -> the root crate.
/// The crate index is loaded once per plan (0.3.46: once per container
/// made deny-sync 35s of codex's validate).
fn crate_of(
    root: &Path,
    index: &crate::search::digest::crates::CrateIndex,
    container_path: &str,
) -> anyhow::Result<Option<String>> {
    // 0.3.44: Cargo's own declarations first (crates with `lib.rs` at the
    // root have no `/src` to split on).
    if let Some((c, _)) = index.locate_dir(container_path) {
        return Ok(Some(c.package.clone()));
    }
    let crate_dir = if let Some((before, _)) = container_path.split_once("/src/") {
        before.to_string()
    } else if let Some(before) = container_path.strip_suffix("/src") {
        before.to_string()
    } else {
        return Ok(None);
    };
    let cargo = root.join(&crate_dir).join("Cargo.toml");
    if !cargo.is_file() {
        return Ok(None);
    }
    let src = fs::read_to_string(&cargo).with_context(|| format!("reading {}", cargo.display()))?;
    let value: toml::Value =
        toml::from_str(&src).with_context(|| format!("parsing {}", cargo.display()))?;
    Ok(value
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(String::from))
}

/// Render the plan as a cargo-deny-compatible deny.toml.
pub fn render_toml(plan: &DenyPlan) -> String {
    let mut out = String::new();
    out.push_str("# Generated by harna from .harna/manifest.toml (compile-time layering).\n");
    out.push_str("# Regenerate with `harna deny` — do not hand-edit.\n\n");
    out.push_str("[bans]\nmultiple-versions = \"warn\"\n");
    out.push_str("deny = [\n");
    for r in &plan.rules {
        let wrapper_list = if r.wrappers.is_empty() {
            String::new()
        } else {
            format!(
                "wrappers = [{}], ",
                r.wrappers
                    .iter()
                    .map(|w| format!("\"{w}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        out.push_str(&format!(
            "  {{ crate = \"{}\", {}reason = \"{}\" }},\n",
            r.crate_name, wrapper_list, r.reason
        ));
    }
    out.push_str("]\n");
    out
}

/// Generate `deny.toml` at the repo root. Idempotent: re-runs overwrite.
pub fn generate(root: &Path, manifest: &Manifest) -> anyhow::Result<std::path::PathBuf> {
    let plan = plan(root, manifest)?;
    let path = root.join("deny.toml");
    fs::write(&path, render_toml(&plan))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Component, ComponentKind, Container, Layer};

    fn manifest() -> Manifest {
        Manifest {
            schema_version: 1,
            layers: vec![
                Layer {
                    isolated: false,
                    name: "engine".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    role: None,
                    containers: vec![Container {
                        escapes: Vec::new(),
                        owns: Vec::new(),
                        name: "core".into(),
                        path: "crates/engine/src/core".into(),
                        description: String::new(),
                        sequence: vec![],
                        provides: vec![],
                        role: None,
                        gateway: None,
                        connects: vec![],
                        components: vec![Component {
                            planned: false,
                            name: "c1".into(),
                            kind: ComponentKind::Primitive,
                            path: String::new(),
                            summary: String::new(),
                            tests: vec![],
                            provides: vec![],
                            children: vec![],
                        }],
                    }],
                },
                Layer {
                    isolated: false,
                    name: "interface".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec!["engine".into()],
                    role: None,
                    containers: vec![
                        Container {
                            escapes: Vec::new(),
                            owns: Vec::new(),
                            name: "cli".into(),
                            path: "crates/cli/src/bin".into(),
                            description: String::new(),
                            sequence: vec![],
                            provides: vec![],
                            role: None,
                            gateway: None,
                            connects: vec![],
                            components: vec![],
                        },
                        Container {
                            escapes: Vec::new(),
                            owns: Vec::new(),
                            name: "mcp".into(),
                            path: "crates/mcp/src/bin".into(),
                            description: String::new(),
                            sequence: vec![],
                            provides: vec![],
                            role: None,
                            gateway: None,
                            connects: vec![],
                            components: vec![],
                        },
                    ],
                },
            ],
        }
    }

    fn write_crate(root: &Path, dir: &str, name: &str) {
        let p = root.join(dir).join("Cargo.toml");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
        )
        .unwrap();
    }

    #[test]
    fn plan_derives_wrappers_from_layer_topography() {
        let tmp = std::env::temp_dir().join(format!("harna-deny-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        write_crate(&tmp, "crates/engine", "engine-lib");
        write_crate(&tmp, "crates/cli", "cli-lib");
        write_crate(&tmp, "crates/mcp", "mcp-lib");

        let plan = plan(&tmp, &manifest()).unwrap();
        let core = plan
            .rules
            .iter()
            .find(|r| r.crate_name == "engine-lib")
            .unwrap();
        // interface declares depends_on = ["engine"], so both interface
        // crates may depend on engine-lib.
        assert_eq!(core.wrappers, vec!["cli-lib", "mcp-lib"]);

        let cli = plan
            .rules
            .iter()
            .find(|r| r.crate_name == "cli-lib")
            .unwrap();
        // Same-layer crates may depend on each other (peers allowed within
        // the layer); nobody outside declared interface depends on cli.
        assert_eq!(cli.wrappers, vec!["mcp-lib"]);

        // And the rendered file parses as TOML with the rules in it.
        let rendered = render_toml(&plan);
        let value: toml::Value = toml::from_str(&rendered).unwrap();
        let deny = value
            .get("bans")
            .unwrap()
            .get("deny")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(deny.len(), 3);
    }

    #[test]
    fn generate_writes_deny_toml() {
        let tmp = std::env::temp_dir().join(format!("harna-deny-w-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        write_crate(&tmp, "crates/engine", "engine-lib");
        let path = generate(&tmp, &manifest()).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("engine-lib"));
    }
}
