//! Component `layering` (0.3.9): the depends_on DAG made strict.
//!
//! Layer ranks: rank(layer) = 1 + max(rank of its depends_on), no
//! depends_on → 0. The rank is the layer's height over the base.
//!
//! - `no-bypass`: an import from rank r to rank ≤ r-2 is a bypass — it
//!   skips the intermediate layer's data rules. Gated by rule data like
//!   budgets: a rule bound to `no-bypass` switches it on (Error by
//!   default there); without such a rule it stays silent, because
//!   hub-shaped layers (an interface reaching everything) are legitimate
//!   and a blanket Error would flood them.
//! - `depth-budget`: how far an import is allowed to reach down the
//!   ladder; the budget is the `limit` on a rule bound to the check
//!   (registry Info default — no rule, no gate).
//!
//! A depends_on cycle cannot be ranked — reported once (visibly dead
//! ranks) instead of silently skipping the whole check.

use std::collections::BTreeMap;
use std::path::Path;

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::model::manifest::Manifest;
use crate::schema::model::rules::Rules;
use crate::search::digest::{crate_name_dirs, resolve_dep, workspace_crate_names, Digest};

pub fn check(
    root: &Path,
    manifest: &Manifest,
    rules: &Rules,
    env: Option<&str>,
    digest: Option<&Digest>,
    out: &mut Vec<Violation>,
) {
    let Some(d) = digest else { return };
    if manifest.is_empty() {
        return;
    }
    let ranks = rank_layers(manifest, out);

    // digest module path -> layer name (shared helper — hardening 0.3.10:
    // the same map transit corridors read, so the two agree by construction).
    let module_layer = super::module_layers(manifest, d);

    let crate_names = workspace_crate_names(root);
    let name_dirs = crate_name_dirs(
        root,
        &d.modules
            .iter()
            .filter_map(|m| m.crate_of().0)
            .filter(|dir| dir != ".")
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>(),
    );
    let hosts = crate::search::digest::HostIndex::new(d);
    // Rule-gated adoption: no rule bound to a bypass check = no gate
    // (hub layers are legitimate; policy stays data).
    let bypass_on = rules.rules.iter().any(|r| {
        !r.is_deprecated() && r.matches_env(env) && r.check.as_deref() == Some("no-bypass")
    });
    for m in &d.modules {
        let Some(importer_layer) = module_layer.get(m.path.as_str()).copied() else {
            continue;
        };
        let Some(importer_rank) = ranks.get(importer_layer).copied() else {
            continue;
        };
        let (importer_dir, stem) = m.crate_of();
        for dep in &m.deps {
            let Some(target) = resolve_dep(dep, &stem, &crate_names) else {
                continue;
            };
            // Resolve the dep's target module to its layer (shared host
            // machinery — longest hosting module wins).
            let Some(target_module) = hosts.hosted(&name_dirs, importer_dir.as_deref(), &target)
            else {
                continue;
            };
            let Some(target_layer) = module_layer.get(target_module.path.as_str()).copied() else {
                continue;
            };
            if target_layer == importer_layer {
                continue; // intra-module steps don't traverse the ladder
            }
            let Some(target_rank) = ranks.get(target_layer).copied() else {
                continue;
            };
            let distance = importer_rank.saturating_sub(target_rank);
            if distance == 0 {
                continue; // sideways or upward imports are other checks' business
            }
            if distance >= 2 && bypass_on {
                out.push(Violation {
                    check: "no-bypass".into(),
                    file: m.path.clone(),
                    line: 0,
                    severity: severity_for("no-bypass"),
                    message: format!(
                        "import `{dep}` bypasses layer(s): `{importer_layer}` (rank {importer_rank}) reaches `{target_layer}` (rank {target_rank}) directly"
                    ),
                    hint: "route through the intermediate layer's own API, or declare the reach: depends_on on the target layer".to_string(),
                });
            }
            if let Some(limit) = depth_limit(rules, env, &m.path) {
                if distance > limit {
                    out.push(Violation {
                        check: "depth-budget".into(),
                        file: m.path.clone(),
                        line: 0,
                        severity: severity_for("depth-budget"),
                        message: format!(
                            "import `{dep}` reaches {distance} rank(s) down — depth budget {limit} (rules.toml)"
                        ),
                        hint: "raise the `limit` on the `depth-budget` rule or sit the importer closer to the data".to_string(),
                    });
                }
            }
        }
    }
}

fn depth_limit(rules: &Rules, env: Option<&str>, path: &str) -> Option<u32> {
    rules
        .rules
        .iter()
        .filter(|r| !r.is_deprecated() && r.matches_env(env))
        .filter(|r| r.check.as_deref() == Some("depth-budget") && r.limit.is_some())
        .filter(|r| {
            r.scope.is_empty()
                || r.scope
                    .iter()
                    .any(|s| crate::schema::model::scope::path_within(path, s))
        })
        .map(|r| r.limit.unwrap())
        .min()
}

fn severity_for(check_id: &str) -> Severity {
    checks::spec(check_id)
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Info)
}

/// rank(layer) = 1 + max(rank of depends_on), 0 when it has none;
/// cyclic members stay unranked (None) and are reported once.
/// `pub(crate)`: the `gateway-ownership` base-facility (`gateway = "base"`)
/// ranks layers with the same DAG read (0.3.10) — one rank derivation.
pub(crate) fn rank_layers<'m>(
    manifest: &'m Manifest,
    out: &mut Vec<Violation>,
) -> BTreeMap<&'m str, u32> {
    let mut ranks: BTreeMap<&str, u32> = BTreeMap::new();
    let mut unranked: std::collections::BTreeSet<&str> =
        manifest.layers.iter().map(|l| l.name.as_str()).collect();
    let names: std::collections::BTreeSet<&str> = unranked.clone();

    let mut settled = false;
    while !settled && !unranked.is_empty() {
        settled = true;
        for l in &manifest.layers {
            if !unranked.contains(l.name.as_str()) {
                continue;
            }
            let deps: Vec<&str> = l
                .depends_on
                .iter()
                .filter(|d| names.contains(d.as_str()))
                .map(|d| d.as_str())
                .collect();
            if l.depends_on.contains(&l.name.to_string()) {
                continue; // self-dependency can never settle
            }
            if deps.iter().all(|d| ranks.contains_key(*d)) {
                let r = deps.iter().map(|d| ranks[*d]).max().map_or(0, |m| m + 1);
                ranks.insert(l.name.as_str(), r);
                unranked.remove(l.name.as_str());
                settled = false;
            }
        }
    }
    if !unranked.is_empty() {
        out.push(Violation {
            check: "manifest-depends".into(),
            file: ".harna/manifest.toml".into(),
            line: 0,
            severity: Severity::Info,
            message: format!(
                "depends_on cycle: {} cannot be ranked (strict layering checks skip these layers until the cycle breaks)",
                unranked.iter().copied().collect::<Vec<_>>().join(", ")
            ),
            hint: "break the cycle in `depends_on` — bases do not depend on their towers".into(),
        });
    }
    ranks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};
    use crate::schema::model::rules::Rule;
    use crate::search::digest::{Digest, Module};

    fn module(path: &str, deps: &[&str]) -> Module {
        Module {
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

    fn manifest3() -> Manifest {
        let container = |path: &str| Container {
            escapes: Vec::new(),
            owns: Vec::new(),
            name: "work".into(),
            path: path.into(),
            description: String::new(),
            sequence: vec![],
            provides: vec![],
            role: None,
            gateway: None,
            connects: vec![],
            components: vec![],
        };
        Manifest {
            schema_version: 1,
            layers: vec![
                Layer {
                    name: "base".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    containers: vec![container("crates/t/src/base")],
                    role: None,
                },
                Layer {
                    name: "middle".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec!["base".into()],
                    containers: vec![container("crates/t/src/mid")],
                    role: None,
                },
                Layer {
                    name: "top".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec!["middle".into()],
                    containers: vec![container("crates/t/src/top")],
                    role: None,
                },
            ],
        }
    }

    fn digest_of(modules: Vec<Module>) -> Digest {
        Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules,
        }
    }

    #[test]
    fn ranks_rise_over_base_and_no_bypass_flags_two_rank_jumps() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "harna-layer-{}-{}",
            std::process::id(),
            std::thread::current()
                .name()
                .unwrap_or("layer")
                .replace("::", "__")
        ));
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

        let manifest = manifest3();
        let d = digest_of(vec![
            // top imports middle's module: one rank (legal).
            module("crates/t/src/top/mod.rs", &["crate::mid::thing"]),
            // top imports base directly: two ranks -> bypass.
            module("crates/t/src/top/cheat.rs", &["crate::base::core"]),
            module("crates/t/src/mid/thing.rs", &[]),
            module("crates/t/src/base/core.rs", &[]),
        ]);

        // No rule bound to no-bypass: silent (hub layers are legitimate).
        let mut v = Vec::new();
        check(&root, &manifest, &Rules::default(), None, Some(&d), &mut v);
        assert!(v.iter().all(|x| x.check != "no-bypass"));

        // A bound rule switches the gate on.
        let mut rules = Rules::default();
        rules.rules.push(no_bypass_rule());
        let mut out = Vec::new();
        check(&root, &manifest, &rules, None, Some(&d), &mut out);
        let bypass: Vec<_> = out.iter().filter(|v| v.check == "no-bypass").collect();
        assert_eq!(bypass.len(), 1);
        assert!(bypass[0].file.ends_with("cheat.rs"));
        assert!(bypass[0].message.contains("(rank 2)") && bypass[0].message.contains("(rank 0)"));
        Ok(())
    }

    fn no_bypass_rule() -> Rule {
        Rule {
            values: Vec::new(),
            id: "no-bypass".into(),
            text: "imports reach at most one rank down the ladder".into(),
            rationale: "towers skip their intermediate's data rules".into(),
            scope: vec![],
            tags: vec![],
            origin: "test".into(),
            examples: vec![],
            env: None,
            check: Some("no-bypass".into()),
            severity: None,
            pattern: None,
            limit: None,
            status: None,
        }
    }

    #[test]
    fn depth_budget_counts_rank_distance_with_rule_limits() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "harna-depth-{}-{}",
            std::process::id(),
            std::thread::current()
                .name()
                .unwrap_or("depth")
                .replace("::", "__")
        ));
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

        let manifest = manifest3();
        let d = digest_of(vec![
            module("crates/t/src/top/mod.rs", &["crate::base::core"]),
            module("crates/t/src/base/core.rs", &[]),
        ]);
        // No rule: silent.
        let mut v = Vec::new();
        check(&root, &manifest, &Rules::default(), None, Some(&d), &mut v);
        assert!(v.is_empty());

        // depth-budget 1 on a scoped rule: the same 2-rank import also trips.
        let mut rules = Rules::default();
        rules.rules.push(Rule {
            values: Vec::new(),
            id: "depth1".into(),
            text: "reach at most one rank".into(),
            rationale: "towers lean".into(),
            scope: vec![],
            tags: vec![],
            origin: "test".into(),
            examples: vec![],
            env: None,
            check: Some("depth-budget".into()),
            severity: None,
            pattern: None,
            limit: Some(1),
            status: None,
        });
        let mut v2 = Vec::new();
        check(&root, &manifest, &rules, None, Some(&d), &mut v2);
        assert!(v2
            .iter()
            .any(|x| x.check == "depth-budget" && x.message.contains("budget 1")));
        Ok(())
    }

    #[test]
    fn cyclic_depends_layers_stay_dead_and_visible() {
        let mut m = manifest3();
        m.layers
            .iter_mut()
            .for_each(|l| l.depends_on = vec![l.name.to_string()]);
        let mut out = Vec::new();
        let ranks = rank_layers(&m, &mut out);
        assert!(ranks.is_empty());
        assert_eq!(out.len(), 1);
        assert!(out[0].message.contains("depends_on cycle"));
        assert_eq!(out[0].severity, Severity::Info);
    }
}
