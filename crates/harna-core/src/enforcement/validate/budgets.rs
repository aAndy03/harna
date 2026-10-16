//! Component `budgets` (0.3.7): structural budgets as rule data.
//!
//! `container-size` — a container stays bounded in components: a rule
//! bound to the check carries its budget in `limit`; violated when the
//! container hosts more digest modules than the strictest applicable
//! budget.
//! `fan-out-cap` — a component stays bounded in outgoing imports: same
//! limit discipline, per module (distinct resolved targets).
//!
//! Both default to registry Info (a budget crossing is guidance, not a
//! gate) — severity overrides promote them per rule; and both stay silent
//! when no rule supplies a limit, so adopting budgets is per-scope data,
//! never a hidden global.

use std::collections::BTreeSet;
use std::path::Path;

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::manifest::Manifest;
use crate::schema::rules::Rules;
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
    let Some(d) = digest else { return };
    let applicable = rules
        .rules
        .iter()
        .filter(|r| !r.is_deprecated() && r.matches_env(env));

    // container-size: budget per container (min of applicable limits).
    for l in &manifest.layers {
        for c in &l.containers {
            if c.path.is_empty() {
                continue;
            }
            let Some(limit) = applicable
                .clone()
                .filter(|r| r.check.as_deref() == Some("container-size") && r.limit.is_some())
                .filter(|r| scope_covers(r, &c.path))
                .map(|r| r.limit.unwrap())
                .min()
            else {
                continue;
            };
            let hosted: Vec<&str> = d
                .modules
                .iter()
                .filter(|m| m.path.starts_with(&format!("{}/", c.path)))
                .map(|m| m.path.as_str())
                .collect();
            if hosted.is_empty() || hosted.len() <= limit as usize {
                continue;
            }
            out.push(Violation {
                check: "container-size".into(),
                file: c.path.to_string(),
                line: 0,
                severity: severity_for("container-size"),
                message: format!(
                    "container `{}` (layer {}) hosts {} module(s) — budget {limit} (rules.toml)",
                    c.name, l.name, hosted.len()
                ),
                hint: format!(
                    "split into sibling containers (register them in the manifest) or raise the `limit` on the `container-size` rule"
                ),
            });
        }
    }

    // fan-out-cap: budget per component (distinct HOSTED import targets —
    // two symbol imports of the same module are one target).
    let crate_names = workspace_crate_names(root);
    // One canonical crate-dir index (hardening 0.3.10: 0.3.7/0.3.9 each
    // hand-rolled this; the shared read cannot diverge between checks).
    let name_dirs = crate_name_dirs(
        root,
        &d.modules
            .iter()
            .filter_map(|m| crate_of(&m.path).0)
            .filter(|dir| dir != ".")
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>(),
    );
    for m in &d.modules {
        let Some(limit) = applicable
            .clone()
            .filter(|r| r.check.as_deref() == Some("fan-out-cap") && r.limit.is_some())
            .filter(|r| scope_covers(r, &m.path))
            .map(|r| r.limit.unwrap())
            .min()
        else {
            continue;
        };
        let (importer_dir, stem) = crate_of(&m.path);
        let importer_dir = importer_dir.unwrap_or_else(|| ".".into());
        let mut targets: BTreeSet<String> = BTreeSet::new();
        for dep in &m.deps {
            let Some(t) = resolve_dep(dep, &stem, &crate_names) else {
                continue;
            };
            // Distinct targets are distinct host modules; an unhosted dep
            // keeps its own resolved stem — visible, not dropped (the `?`
            // fence avoids colliding with real module paths).
            let key = match hosted_module(d, &name_dirs, Some(importer_dir.as_str()), &t) {
                Some(hosted) => hosted.path.clone(),
                None => format!(
                    "?{}::{}?",
                    t.external.as_deref().unwrap_or(&importer_dir),
                    t.stem
                ),
            };
            targets.insert(key);        }
        if targets.len() <= limit as usize {
            continue;
        }
        out.push(Violation {
            check: "fan-out-cap".into(),
            file: m.path.clone(),
            line: 0,
            severity: severity_for("fan-out-cap"),
            message: format!(
                "module imports {n} distinct target(s) — budget {limit} (rules.toml)",
                n = targets.len()
            ),
            hint: "collect the fan-out behind one facade, split the module, or raise the `limit` on the `fan-out-cap` rule".into(),
        });
    }
}

fn scope_covers(rule: &crate::schema::rules::Rule, path: &str) -> bool {
    rule.scope.is_empty() || rule.scope.iter().any(|s| path.starts_with(s.as_str()))
}

fn severity_for(check_id: &str) -> Severity {
    checks::spec(check_id)
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::manifest::{Container, Layer};
    use crate::schema::rules::Rule;
    use crate::search::digest::Module;

    fn module(path: &str, deps: &[&str]) -> Module {
        Module {
            path: path.into(),
            lang: "rust".into(),
            signatures: vec![],
            deps: deps.iter().map(|s| s.to_string()).collect(),
            body_hash: "x".into(),
            inventory: vec![],
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

    fn manifest_one_container(path: &str) -> Manifest {
        Manifest {
            schema_version: 1,
            layers: vec![Layer {
                name: "engine".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                containers: vec![Container {
                    name: "core".into(),
                    path: path.into(),
                    description: String::new(),
                    sequence: vec![],
                    provides: vec![],
                    role: None,
                    gateway: None,
                    connects: vec![],
                    components: vec![],
                }],
            }],
        }
    }

    fn budget_rule(id: &str, check_id: &str, limit: u32, scope: &[&str]) -> Rule {
        Rule {
            id: id.into(),
            text: format!("budget {check_id}"),
            rationale: "growth visibility".into(),
            scope: scope.iter().map(|s| s.to_string()).collect(),
            tags: vec![],
            origin: "test".into(),
            examples: vec![],
            env: None,
            check: Some(check_id.into()),
            severity: None,
            pattern: None,
            limit: Some(limit),
            status: None,
        }
    }

    #[test]
    fn container_size_budget_flags_overage_only_with_a_declared_limit() {
        let manifest = manifest_one_container("crates/t/src/engine");
        let mut mods = Vec::new();
        for i in 0..4 {
            mods.push(module(&format!("crates/t/src/engine/m{i}.rs"), &[]));
        }
        let d = digest_of(mods);

        // Budget 3: 4 modules inside -> one advisory.
        let mut rules = Rules::default();
        rules.rules.push(budget_rule("size3", "container-size", 3, &[]));
        let mut v = Vec::new();
        check(Path::new("."), &manifest, &rules, None, Some(&d), &mut v);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].check, "container-size");
        assert_eq!(v[0].severity, Severity::Info, "guidance by default");
        assert!(v[0].message.contains("hosts 4 module(s) — budget 3"));

        // Four allowed: silent.
        let mut rules4 = Rules::default();
        rules4.rules.push(budget_rule("size4", "container-size", 4, &[]));
        let mut v2 = Vec::new();
        check(Path::new("."), &manifest, &rules4, None, Some(&d), &mut v2);
        assert!(v2.is_empty());

        // No budget declared at all: silent (adoption is data, not default).
        let mut empty = Vec::new();
        check(Path::new("."), &manifest, &Rules::default(), None, Some(&d), &mut empty);
        assert!(empty.is_empty());
    }

    #[test]
    fn strictest_applicable_limit_wins_and_scope_scopes() {
        let manifest = manifest_one_container("crates/t/src/engine");
        let modules = (0..5u32)
            .map(|i| module(&format!("crates/t/src/engine/m{i}.rs"), &[]))
            .collect();
        let d = digest_of(modules);

        let mut rules = Rules::default();
        rules.rules.push(budget_rule("loose", "container-size", 9, &[]));
        rules.rules.push(budget_rule("tight", "container-size", 2, &[]));
        let mut v = Vec::new();
        check(Path::new("."), &manifest, &rules, None, Some(&d), &mut v);
        assert!(v[0].message.contains("budget 2"), "min wins, was {}", v[0].message);

        // A scope that does not cover the container exempts its rule.
        let mut scoped = Rules::default();
        scoped.rules.push(budget_rule("tight", "container-size", 2, &["other/path"]));
        scoped.rules.push(budget_rule("loose", "container-size", 9, &[]));
        let mut v2 = Vec::new();
        check(Path::new("."), &manifest, &scoped, None, Some(&d), &mut v2);
        assert!(v2.is_empty(), "9 modules <= 9 budget; out-of-scope 2 ignored");
    }

    #[test]
    fn fan_out_cap_counts_distinct_targets_per_module() {
        let d = digest_of(vec![
            module(
                "crates/t/src/hub.rs",
                &[
                    "crate::one::x",
                    "crate::one::y",
                    "crate::two::z",
                    "crate::three::w",
                ],
            ),
            // The dep targets host their symbols; two symbol imports into
            // `one` collapse to one target.
            module("crates/t/src/one.rs", &[]),
            module("crates/t/src/two.rs", &[]),
            module("crates/t/src/three.rs", &[]),
        ]);
        let empty_manifest = Manifest::default();
        let mut rules = Rules::default();
        rules.rules.push(budget_rule("cap", "fan-out-cap", 2, &[]));
        let mut v = Vec::new();
        check(Path::new("."), &empty_manifest, &rules, None, Some(&d), &mut v);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("imports 3 distinct target(s) — budget 2"),
            "4 deps collapse to 3 hosted targets, was {}", v[0].message);
    }

    #[test]
    fn deprecated_rules_owe_no_budget() {
        let manifest = manifest_one_container("crates/t/src/engine");
        let modules = (0..4u32)
            .map(|i| module(&format!("crates/t/src/engine/m{i}.rs"), &[]))
            .collect();
        let d = digest_of(modules);
        let mut rules = Rules::default();
        let mut r = budget_rule("dead", "container-size", 3, &[]);
        r.status = Some("deprecated".into());
        rules.rules.push(r);
        let mut v = Vec::new();
        check(Path::new("."), &manifest, &rules, None, Some(&d), &mut v);
        assert!(v.is_empty(), "a dead rule cannot cap");
    }
}
