//! Component `transit` (0.3.10): transiter corridors as manifest data.
//!
//! A container declares the corridor it mediates with `connects =
//! ["api", "billing"]`. Declaring a corridor is the adoption switch (data,
//! never a hidden default): until a corridor is named, cross-layer imports
//! are `layer-boundary`'s business; once it is named, two things become
//! true —
//!
//! - `no-sideways` — a DIRECT cross-layer import between the connected
//!   layers bypasses the transiter: the corridor exists exactly so the
//!   coupling shapes itself through a thin pass-through, so an edge that
//!   skips it is a violation (Error). Imports landing inside the
//!   transiter, the transiter's own outgoing imports, and pairs no
//!   corridor names stay silent — no corridor, no gate. A declared
//!   `depends_on` does not exempt: the corridor is the stricter, more
//!   deliberate shape (declaring a transiter and a same-moment direct
//!   edge at once calls two different architectures true).
//! - `transiter-thinness` — the transiter stays thin: declared items +
//!   signatures per module under the `limit` carried by a rule bound to
//!   the check. No rule, no gate (the 0.3.7 budget discipline): thinness
//!   is adopted as data, and the strictest applicable limit wins.
//!
//! Both graph halves read the committed map (stale-data honesty: they are
//! as correct as the last `harna scan`).

use std::collections::HashMap;
use std::path::Path;

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::model::manifest::{Component, Manifest};
use crate::schema::model::rules::{Rule, Rules};
use crate::search::digest::{
    crate_name_dirs, crate_of, hosted_module, resolve_dep, workspace_crate_names, Digest,
};

pub fn check(
    root: &Path,
    manifest: &Manifest,
    rules: &Rules,
    env: Option<&str>,
    digest: Option<&Digest>,
    out: &mut Vec<Violation>,
) {
    check_declarations(manifest, out); // cheap: even a digest-less run names corridor typos
    let Some(d) = digest else { return };
    let corridors = corridors(manifest);
    if corridors.is_empty() {
        return; // no declared transiter: cross-layer imports stay layer-boundary's business
    }
    check_no_sideways(root, manifest, &corridors, d, out);
    check_thinness(manifest, rules, env, &corridors, d, out);
}

/// Declared corridors: `(container path, [layer names])`, manifest order.
fn corridors(manifest: &Manifest) -> Vec<(&str, Vec<&str>)> {
    manifest
        .layers
        .iter()
        .flat_map(|l| l.containers.iter().map(|c| (c.path.as_str(), &c.connects)))
        .filter(|(_, connects)| !connects.is_empty())
        .map(|(path, connects)| (path, connects.iter().map(|s| s.as_str()).collect()))
        .collect()
}

/// Declaration coherence (Info): corridor names that are not layers, a
/// corridor with fewer than two distinct layers, and a corridor naming its
/// own layer. Hand-edited data stays visible, never silently half-ignored.
pub fn check_declarations(manifest: &Manifest, out: &mut Vec<Violation>) {
    let known: std::collections::BTreeSet<&str> =
        manifest.layers.iter().map(|l| l.name.as_str()).collect();
    for layer in &manifest.layers {
        for c in &layer.containers {
            if c.connects.is_empty() {
                continue;
            }
            for name in &c.connects {
                if !known.contains(name.as_str()) {
                    out.push(Violation {
                        check: "no-sideways".into(),
                        file: ".harna/manifest.toml".into(),
                        line: 0,
                        severity: Severity::Info,
                        message: format!(
                            "container `{}` declares a corridor to unknown layer `{name}`",
                            c.name
                        ),
                        hint:
                            "corridor names are layer names: fix the spelling or create the layer"
                                .into(),
                    });
                }
                if layer.name == *name {
                    out.push(Violation {
                        check: "no-sideways".into(),
                        file: ".harna/manifest.toml".into(),
                        line: 0,
                        severity: Severity::Info,
                        message: format!(
                            "container `{}` corridors its own layer `{name}` — a transiter mediates between different layers",
                            c.name
                        ),
                        hint: "drop the own-layer name from `connects`, or give the transiter its own layer".into(),
                    });
                }
            }
            let distinct = c.connects.iter().collect::<std::collections::BTreeSet<_>>();
            if distinct.len() < 2 {
                out.push(Violation {
                    check: "no-sideways".into(),
                    file: ".harna/manifest.toml".into(),
                    line: 0,
                    severity: Severity::Info,
                    message: format!(
                        "container `{}` declares a corridor of {} — a transiter connects at least two layers",
                        c.name,
                        c.connects.len()
                    ),
                    hint: "write the two (or more) layers this transiter mediates between".into(),
                });
            }
        }
    }
}

/// Direct cross-layer edges between corridor-declared layers bypass the
/// transiter; edges landing inside a corridor container are the corridor
/// working as declared.
fn check_no_sideways(
    root: &Path,
    manifest: &Manifest,
    corridors: &[(&str, Vec<&str>)],
    d: &Digest,
    out: &mut Vec<Violation>,
) {
    let homes = super::module_homes(manifest, d);
    let corridor_paths: std::collections::BTreeSet<&str> =
        corridors.iter().map(|(p, _)| *p).collect();
    let mut names_by_path: HashMap<&str, &str> = HashMap::new();
    for l in &manifest.layers {
        for c in &l.containers {
            names_by_path
                .entry(c.path.as_str())
                .or_insert(c.name.as_str());
        }
    }
    // Pair index: (from layer, to layer) -> naming corridor container paths.
    let mut pairs: HashMap<(String, String), Vec<&str>> = HashMap::new();
    for (path, names) in corridors {
        for i in 0..names.len() {
            for j in 0..names.len() {
                if i == j || names[i] == names[j] {
                    continue;
                }
                pairs
                    .entry((names[i].to_string(), names[j].to_string()))
                    .or_default()
                    .push(path);
            }
        }
    }
    let crate_names = workspace_crate_names(root);
    let dirs: std::collections::BTreeSet<String> = d
        .modules
        .iter()
        .filter_map(|m| crate_of(&m.path).0)
        .filter(|dir| dir != ".")
        .collect();
    let name_dirs = crate_name_dirs(root, &dirs.into_iter().collect::<Vec<_>>());
    let mut reported: std::collections::BTreeSet<(&str, &str)> = std::collections::BTreeSet::new();
    for m in &d.modules {
        let Some(home) = homes.get(m.path.as_str()) else {
            continue;
        };
        if corridor_paths.contains(home.container_path) {
            continue; // the transiter bridges its own corridor by design
        }
        let (importer_dir, stem) = crate_of(&m.path);
        for dep in &m.deps {
            let Some(target) = resolve_dep(dep, stem.as_str(), &crate_names) else {
                continue;
            };
            let Some(hosted) = hosted_module(d, &name_dirs, importer_dir.as_deref(), &target)
            else {
                continue;
            };
            let Some(t_home) = homes.get(hosted.path.as_str()) else {
                continue;
            };
            if t_home.layer == home.layer {
                continue; // steps inside one layer are other checks' business
            }
            let Some(via) = pairs.get(&(home.layer.to_string(), t_home.layer.to_string())) else {
                continue; // no corridor for this pair: nothing is gated yet
            };
            if corridor_paths.contains(t_home.container_path) {
                continue; // through the transiter: exactly what the corridor asks
            }
            if reported.insert((m.path.as_str(), dep.as_str())) {
                out.push(Violation {
                    check: "no-sideways".into(),
                    file: m.path.clone(),
                    line: 0,
                    severity: severity_for(),
                    message: format!(
                        "import `{dep}` skips the transiter {} ({} -> {}) with a direct edge",
                        via.iter()
                            .map(|p| names_by_path.get(*p).copied().unwrap_or(p))
                            .collect::<Vec<_>>()
                            .join(" / "),
                        home.layer,
                        t_home.layer
                    ),
                    hint: "route through the transiter's API, or re-home the symbol the corridor was declared for".into(),
                });
            }
        }
    }
}

/// Declared transiter modules stay thin: declared items + signatures per
/// module under the bound rule's `limit`; the strictest applicable rule
/// wins, and no rule means no gate (adoption is data). A component path
/// the digest has never seen is the manifest-path check's finding.
fn check_thinness(
    manifest: &Manifest,
    rules: &Rules,
    env: Option<&str>,
    corridors: &[(&str, Vec<&str>)],
    d: &Digest,
    out: &mut Vec<Violation>,
) {
    let by_path: HashMap<&str, &crate::search::digest::Module> =
        d.modules.iter().map(|m| (m.path.as_str(), m)).collect();
    for (container_path, _) in corridors {
        for component in declared_components(manifest, container_path) {
            if component.path.is_empty() {
                continue;
            }
            let Some(module) = by_path.get(component.path.as_str()) else {
                continue;
            };
            let Some(limit) = thin_limit(rules, env, &component.path) else {
                continue;
            };
            let held = module.signatures.len() + module.inventory.len();
            if held <= limit as usize {
                continue;
            }
            out.push(Violation {
                check: "transiter-thinness".into(),
                file: component.path.clone(),
                line: 0,
                severity: thinness_severity(),
                message: format!(
                    "transiter module holds {held} declared item(s) — budget {limit} (rules.toml)"
                ),
                hint: "keep the corridor thin: the logic lives in the engines it bridges; the transiter relays".into(),
            });
        }
    }
}

/// The corridor container's declared components (top level nested too).
fn declared_components<'m>(manifest: &'m Manifest, container_path: &str) -> Vec<&'m Component> {
    let mut found = Vec::new();
    for layer in &manifest.layers {
        for c in &layer.containers {
            if c.path != container_path {
                continue;
            }
            fn push<'m>(comps: &'m [Component], out: &mut Vec<&'m Component>) {
                for comp in comps {
                    out.push(comp);
                    push(&comp.children, out);
                }
            }
            push(&c.components, &mut found);
        }
    }
    found
}

/// The strictest applicable `transiter-thinness` budget.
fn thin_limit(rules: &Rules, env: Option<&str>, path: &str) -> Option<u32> {
    rules
        .rules
        .iter()
        .filter(|r| !r.is_deprecated() && r.matches_env(env))
        .filter(|r| r.check.as_deref() == Some("transiter-thinness") && r.limit.is_some())
        .filter(|r| scope_covers(r, path))
        .map(|r| r.limit.unwrap())
        .min()
}

fn scope_covers(rule: &Rule, path: &str) -> bool {
    rule.scope.is_empty() || rule.scope.iter().any(|s| path.starts_with(s.as_str()))
}

fn severity_for() -> Severity {
    checks::spec("no-sideways")
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Error)
}

fn thinness_severity() -> Severity {
    checks::spec("transiter-thinness")
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{ComponentKind, Container, Layer};
    use crate::search::digest::{Item, Module};

    fn container(name: &str, path: &str, connects: &[&str]) -> Container {
        Container {
            name: name.into(),
            path: path.into(),
            description: String::new(),
            sequence: vec![],
            provides: vec![],
            role: None,
            gateway: None,
            connects: connects.iter().map(|s| s.to_string()).collect(),
            components: vec![],
        }
    }

    fn transit_manifest() -> Manifest {
        Manifest {
            schema_version: 1,
            layers: vec![
                Layer {
                    name: "api".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    containers: vec![container("main", "crates/t/src/api", &[])],
                    role: None,
                },
                Layer {
                    name: "transit".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    role: None,
                    containers: vec![container(
                        "api-billing",
                        "crates/t/src/corridoro",
                        &["api", "billing"],
                    )],
                },
                Layer {
                    name: "billing".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    containers: vec![container("engine", "crates/t/src/billing", &[])],
                    role: None,
                },
            ],
        }
    }

    fn module(path: &str, deps: &[&str], sigs: usize, items: usize) -> Module {
        Module {
            path: path.into(),
            lang: "rust".into(),
            signatures: (0..sigs).map(|i| format!("fn s{i}()")).collect(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            body_hash: "x".into(),
            inventory: (0..items)
                .map(|i| Item {
                    vis: "pub".into(),
                    kind: "struct".into(),
                    name: format!("T{i}"),
                    text: format!("pub struct T{i}"),
                })
                .collect(),
            mods: vec![],
        }
    }

    fn digest_of(modules: Vec<Module>) -> Digest {
        Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules,
        }
    }

    fn thin_rule(id: &str, limit: u32, env: Option<&str>, scope: &[&str]) -> Rule {
        Rule {
            id: id.into(),
            text: format!("transiters stay under {limit}"),
            rationale: "thin corridors".into(),
            scope: scope.iter().map(|s| s.to_string()).collect(),
            tags: vec![],
            origin: "test".into(),
            examples: vec![],
            env: env.map(|s| s.to_string()),
            check: Some("transiter-thinness".into()),
            severity: None,
            pattern: None,
            limit: Some(limit),
            status: None,
        }
    }

    fn workspace() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "harna-transit-{}-{}",
            std::process::id(),
            std::thread::current()
                .name()
                .unwrap_or("t")
                .replace("::", "__")
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/t")).unwrap();
        std::fs::write(
            root.join("crates/t/Cargo.toml"),
            "[package]\nname = \"t\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/t\"]\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn mediated_import_passes_direct_bypass_flags() {
        let root = workspace();
        let m = transit_manifest();
        let d = digest_of(vec![
            module(
                "crates/t/src/api/h.rs",
                &["crate::corridoro::relay", "crate::billing::charger"],
                1,
                0,
            ),
            module(
                "crates/t/src/corridoro/relay.rs",
                &["crate::billing::charger"],
                1,
                0,
            ),
            module("crates/t/src/billing/charger.rs", &[], 1, 0),
        ]);
        let mut out = Vec::new();
        check(&root, &m, &Rules::default(), None, Some(&d), &mut out);
        let hits: Vec<_> = out.iter().filter(|v| v.check == "no-sideways").collect();
        assert_eq!(hits.len(), 1, "only the direct edge bypasses, was: {out:?}");
        assert!(hits[0].file.ends_with("api/h.rs"));
        assert_eq!(hits[0].severity, Severity::Error);
        assert!(hits[0]
            .message
            .contains("skips the transiter api-billing (api -> billing)"));
        assert!(hits[0].message.contains("crate::billing::charger"));
    }

    #[test]
    fn declared_depends_on_does_not_nullify_the_corridor() -> anyhow::Result<()> {
        let root = workspace();
        let mut m = transit_manifest();
        m.layers[0].depends_on = vec!["billing".into()]; // legal for layer-boundary
        let d = digest_of(vec![
            module("crates/t/src/api/h.rs", &["crate::billing::charger"], 0, 0),
            module("crates/t/src/billing/charger.rs", &[], 0, 0),
        ]);
        let mut out = Vec::new();
        check(&root, &m, &Rules::default(), None, Some(&d), &mut out);
        assert_eq!(
            out.iter().filter(|v| v.check == "no-sideways").count(),
            1,
            "the corridor is the stricter shape; a declared direct edge still bypasses, was: {out:?}"
        );
        Ok(())
    }

    #[test]
    fn undeclared_corridors_gate_nothing() {
        let root = workspace();
        let mut m = transit_manifest();
        for l in &mut m.layers {
            for c in &mut l.containers {
                c.connects.clear();
            }
        }
        let d = digest_of(vec![
            module(
                "crates/t/src/api/h.rs",
                &["crate::corridoro::relay", "crate::billing::charger"],
                0,
                0,
            ),
            module(
                "crates/t/src/corridoro/relay.rs",
                &["crate::billing::charger"],
                0,
                0,
            ),
            module("crates/t/src/billing/charger.rs", &[], 0, 0),
        ]);
        let mut out = Vec::new();
        check(&root, &m, &Rules::default(), None, Some(&d), &mut out);
        assert!(out.is_empty(), "no data, no gate — was: {out:?}");
    }

    #[test]
    fn cross_crate_edges_obey_the_corridor_too() {
        let root = workspace();
        std::fs::create_dir_all(root.join("crates/u")).unwrap();
        std::fs::write(
            root.join("crates/u/Cargo.toml"),
            "[package]\nname = \"u\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/t\", \"crates/u\"]\n",
        )
        .unwrap();
        let mut m = transit_manifest();
        m.layers[0]
            .containers
            .push(container("other", "crates/u/src/api", &[]));
        let d = digest_of(vec![
            // Top imports across into the other crate directly.
            module("crates/u/src/api/h.rs", &["t::billing::charger"], 0, 0),
            // Same import through the corridor: mediated.
            module("crates/u/src/api/gwuse.rs", &["t::corridoro::relay"], 0, 0),
            module("crates/t/src/billing/charger.rs", &[], 0, 0),
            module("crates/t/src/corridoro/relay.rs", &[], 0, 0),
        ]);
        let mut out = Vec::new();
        check(&root, &m, &Rules::default(), None, Some(&d), &mut out);
        let hits: Vec<_> = out.iter().filter(|v| v.check == "no-sideways").collect();
        assert_eq!(
            hits.len(),
            1,
            "cross-crate direct edge bypasses too, was: {out:?}"
        );
        assert!(hits[0].file.ends_with("crates/u/src/api/h.rs"));
    }

    #[test]
    fn corridor_declarations_speak_their_coherence() {
        let mut m = transit_manifest();
        m.layers[1].containers[0].connects = vec!["api".into(), "banana".into(), "transit".into()];
        let mut out = Vec::new();
        check_declarations(&m, &mut out);
        let infos: Vec<_> = out
            .iter()
            .filter(|v| v.severity == Severity::Info)
            .collect();
        assert!(
            infos
                .iter()
                .any(|v| v.message.contains("unknown layer `banana`")),
            "was: {out:?}"
        );
        assert!(
            infos
                .iter()
                .any(|v| v.message.contains("its own layer `transit`")),
            "was: {out:?}"
        );
        // A single-layer corridor is narrated too.
        let mut m2 = transit_manifest();
        m2.layers[1].containers[0].connects = vec!["api".into()];
        let mut out2 = Vec::new();
        check_declarations(&m2, &mut out2);
        assert!(out2.iter().any(|v| v.message.contains("corridor of 1")));
    }

    #[test]
    fn thinness_holds_declared_transit_modules_to_the_rule_budget() {
        let root = workspace();
        let mut m = transit_manifest();
        m.layers[1].containers[0].components.push(Component {
            name: "relay".into(),
            kind: ComponentKind::Primitive,
            path: "crates/t/src/corridoro/relay.rs".into(),
            summary: String::new(),
            tests: vec![],
            provides: vec![],
            children: vec![],
        });
        let d = digest_of(vec![
            module(
                "crates/t/src/corridoro/relay.rs",
                &["crate::billing::charger"],
                5,
                5,
            ),
            module("crates/t/src/billing/charger.rs", &[], 3, 3),
        ]);

        // No rule: silent (thinness is adopted as data).
        let mut out = Vec::new();
        check(&root, &m, &Rules::default(), None, Some(&d), &mut out);
        assert!(out.iter().all(|v| v.check != "transiter-thinness"));

        // Budget 7: the corridor module (10 items) crosses it once.
        let mut rules = Rules::default();
        rules.rules.push(thin_rule("thin7", 7, None, &[]));
        let mut out2 = Vec::new();
        check(&root, &m, &rules, None, Some(&d), &mut out2);
        let hits: Vec<_> = out2
            .iter()
            .filter(|v| v.check == "transiter-thinness")
            .collect();
        assert_eq!(hits.len(), 1, "was: {out2:?}");
        assert!(hits[0].file.ends_with("corridoro/relay.rs"));
        assert!(hits[0]
            .message
            .contains("holds 10 declared item(s) — budget 7"));
        assert_eq!(hits[0].severity, Severity::Info);
        assert!(
            !out2
                .iter()
                .any(|v| v.check == "transiter-thinness" && v.file.ends_with("charger.rs")),
            "the engines' own modules are not transiters"
        );

        // Budget 10: exactly at the line, no crossing.
        let mut rules10 = Rules::default();
        rules10.rules.push(thin_rule("thin10", 10, None, &[]));
        let mut out3 = Vec::new();
        check(&root, &m, &rules10, None, Some(&d), &mut out3);
        assert!(out3.iter().all(|v| v.check != "transiter-thinness"));

        // The strictest applicable limit wins; env-tagged rules stay out of
        // other runs; deprecated rules owe no budget; unhosted components
        // say nothing here (manifest-path owns that finding).
        let mut rules4 = Rules::default();
        let mut dead = thin_rule("dead", 100, None, &[]);
        dead.status = Some("deprecated".into());
        rules4.rules.push(dead);
        rules4.rules.push(thin_rule("loose", 15, Some("dev"), &[]));
        rules4.rules.push(thin_rule("tight", 4, None, &[]));
        let mut out4 = Vec::new();
        check(&root, &m, &rules4, None, Some(&d), &mut out4);
        assert!(
            out4.iter()
                .any(|v| v.check == "transiter-thinness" && v.message.contains("budget 4")),
            "only the in-scope, live, strictest rule gates, was: {out4:?}"
        );
    }
}
