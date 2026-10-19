//! Component `budgets` (0.3.7): structural budgets as rule data.
//!
//! `container-size` — a container stays bounded in components: a rule
//! bound to the check carries its budget in `limit`; violated when the
//! container hosts more digest modules than the strictest applicable
//! budget.
//! `fan-out-cap` — a component stays bounded in outgoing imports: same
//! limit discipline, per module (distinct resolved targets).
//! `born-size` (0.3.26) — a new file arrives inside its budget: of the
//! `.rs` files BORN in the working tree (added / untracked, per
//! `git::new_rust_files`), any that arrive over the strictest applicable
//! budget are errors — split per-scope before the burst's commit. The
//! splitting is the writing; retro-splitting across sessions burns
//! agent context. Files with history are NOT checked here — accretion
//! guidance (`component-granularity`, `hotspot`) advises them instead,
//! so a birth error never punishes an inherited codebase: no
//! grandfathering is needed when fresh work is what's gated.
//!
//! All stay silent when no rule supplies a limit, so adopting budgets
//! is per-scope data, never a hidden global; both default-severities
//! live in the registry (`checks.toml`) — `born-size` gates (error),
//! the shape budgets advise (info), and severity overrides promote
//! per rule.

use std::collections::BTreeSet;
use std::path::Path;

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::model::manifest::Manifest;
use crate::schema::model::rules::Rules;
use crate::search::digest::{
    crate_name_dirs, hosted_module, resolve_dep, workspace_crate_names, Digest,
};

pub fn check(
    root: &Path,
    manifest: &Manifest,
    rules: &Rules,
    env: Option<&str>,
    digest: Option<&Digest>,
    new_files: &[String],
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
                .filter(|m| crate::schema::model::scope::path_in_container(&m.path, &c.path))
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
                hint: "split into sibling containers (register them in the manifest) or raise the `limit` on the `container-size` rule".to_string(),
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
            .filter_map(|m| m.crate_of().0)
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
        let (importer_dir, stem) = m.crate_of();
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
            targets.insert(key);
        }
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

    // born-size: a new file arrives within its budget. Only BORN files
    // are examined (git::new_rust_files); an unreadable birth (deleted
    // mid-burst, or binary) is skipped rather than guessed.
    for file in new_files {
        let Some(limit) = applicable
            .clone()
            .filter(|r| r.check.as_deref() == Some("born-size") && r.limit.is_some())
            .filter(|r| scope_covers(r, file))
            .map(|r| r.limit.unwrap())
            .min()
        else {
            continue;
        };
        let lines = std::fs::read_to_string(root.join(file))
            .map(|src| src.lines().count())
            .unwrap_or(0);
        if lines <= limit as usize {
            continue;
        }
        out.push(Violation {
            check: "born-size".into(),
            file: file.clone(),
            line: 0,
            severity: severity_for("born-size"),
            message: format!(
                "new file born with {lines} lines — born-size budget {limit} (rules.toml)"
            ),
            hint: "split into per-scope modules and register them in the manifest \
                   before the burst's commit (the splitting is the writing, \
                   `harna scaffold` reserves homes); raise the `limit` consciously \
                   if the size is deliberate"
                .into(),
        });
    }
}

/// The strictest `limit` a live rule bound to `check_id` sets over
/// `path` (0.3.43: every numeric budget is rule data — the advisory
/// thresholds included, not only the gating budgets).
pub fn bound_limit(rules: &Rules, env: Option<&str>, check_id: &str, path: &str) -> Option<u32> {
    rules
        .rules
        .iter()
        .filter(|r| !r.is_deprecated() && r.matches_env(env))
        .filter(|r| r.check.as_deref() == Some(check_id))
        .filter(|r| scope_covers(r, path))
        .filter_map(|r| r.limit)
        .min()
}

fn scope_covers(rule: &crate::schema::model::rules::Rule, path: &str) -> bool {
    rule.scope.is_empty()
        || rule
            .scope
            .iter()
            .any(|s| crate::schema::model::scope::path_within(path, s))
}

fn severity_for(check_id: &str) -> Severity {
    checks::spec(check_id)
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};
    use crate::schema::model::rules::Rule;
    use crate::search::digest::Module;

    fn module(path: &str, deps: &[&str]) -> Module {
        Module {
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
                role: None,
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
        rules
            .rules
            .push(budget_rule("size3", "container-size", 3, &[]));
        let mut v = Vec::new();
        check(
            Path::new("."),
            &manifest,
            &rules,
            None,
            Some(&d),
            &[],
            &mut v,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].check, "container-size");
        assert_eq!(v[0].severity, Severity::Info, "guidance by default");
        assert!(v[0].message.contains("hosts 4 module(s) — budget 3"));

        // Four allowed: silent.
        let mut rules4 = Rules::default();
        rules4
            .rules
            .push(budget_rule("size4", "container-size", 4, &[]));
        let mut v2 = Vec::new();
        check(
            Path::new("."),
            &manifest,
            &rules4,
            None,
            Some(&d),
            &[],
            &mut v2,
        );
        assert!(v2.is_empty());

        // No budget declared at all: silent (adoption is data, not default).
        let mut empty = Vec::new();
        check(
            Path::new("."),
            &manifest,
            &Rules::default(),
            None,
            Some(&d),
            &[],
            &mut empty,
        );
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
        rules
            .rules
            .push(budget_rule("loose", "container-size", 9, &[]));
        rules
            .rules
            .push(budget_rule("tight", "container-size", 2, &[]));
        let mut v = Vec::new();
        check(
            Path::new("."),
            &manifest,
            &rules,
            None,
            Some(&d),
            &[],
            &mut v,
        );
        assert!(
            v[0].message.contains("budget 2"),
            "min wins, was {}",
            v[0].message
        );

        // A scope that does not cover the container exempts its rule.
        let mut scoped = Rules::default();
        scoped
            .rules
            .push(budget_rule("tight", "container-size", 2, &["other/path"]));
        scoped
            .rules
            .push(budget_rule("loose", "container-size", 9, &[]));
        let mut v2 = Vec::new();
        check(
            Path::new("."),
            &manifest,
            &scoped,
            None,
            Some(&d),
            &[],
            &mut v2,
        );
        assert!(
            v2.is_empty(),
            "9 modules <= 9 budget; out-of-scope 2 ignored"
        );
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
        check(
            Path::new("."),
            &empty_manifest,
            &rules,
            None,
            Some(&d),
            &[],
            &mut v,
        );
        assert_eq!(v.len(), 1);
        assert!(
            v[0].message
                .contains("imports 3 distinct target(s) — budget 2"),
            "4 deps collapse to 3 hosted targets, was {}",
            v[0].message
        );
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
        check(
            Path::new("."),
            &manifest,
            &rules,
            None,
            Some(&d),
            &[],
            &mut v,
        );
        assert!(v.is_empty(), "a dead rule cannot cap");
    }

    #[test]
    fn born_size_gates_a_new_file_over_budget_and_within_is_silent() {
        let tmp = std::env::temp_dir().join(format!("harna-born-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let big = "pub fn placeholder() {}\n".repeat(50);
        let big_path = tmp.join("src/big.rs");
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::write(&big_path, &big).unwrap();

        let d = digest_of(vec![]);
        let new_files = vec!["src/big.rs".to_string()];
        let rel = tmp.to_string_lossy().to_string();
        let root = Path::new(&rel);

        // A 50-line birth over a 10-line budget is an Error (a gate).
        let mut rules = Rules::default();
        rules
            .rules
            .push(budget_rule("born10", "born-size", 10, &[]));
        let mut v = Vec::new();
        check(
            root,
            &Manifest::default(),
            &rules,
            None,
            Some(&d),
            &new_files,
            &mut v,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].check, "born-size");
        assert_eq!(
            v[0].severity,
            Severity::Error,
            "born checked, a gate not advice"
        );
        assert!(
            v[0].message
                .contains("born with 50 lines — born-size budget 10"),
            "was {}",
            v[0].message
        );

        // Inside the budget: silent.
        let mut rules100 = Rules::default();
        rules100
            .rules
            .push(budget_rule("born100", "born-size", 100, &[]));
        let mut v2 = Vec::new();
        check(
            root,
            &Manifest::default(),
            &rules100,
            None,
            Some(&d),
            &new_files,
            &mut v2,
        );
        assert!(v2.is_empty());

        // No `born-size` rule at all: silent (adoption is data).
        let mut v3 = Vec::new();
        check(
            root,
            &Manifest::default(),
            &Rules::default(),
            None,
            Some(&d),
            &new_files,
            &mut v3,
        );
        assert!(v3.is_empty());

        // A file with history is NOT a birth: the same oversized content
        // under a modified status never fires born-size (accretion advises
        // it instead) — a birth error cannot punish inherited code.
        let mut v4 = Vec::new();
        check(
            root,
            &Manifest::default(),
            &rules,
            None,
            Some(&d),
            &[],
            &mut v4,
        );
        assert!(v4.is_empty());

        // Strictest applicable limit wins.
        let mut rules12 = Rules::default();
        rules12
            .rules
            .push(budget_rule("born10", "born-size", 10, &[]));
        rules12
            .rules
            .push(budget_rule("born45", "born-size", 45, &[]));
        let mut v5 = Vec::new();
        check(
            root,
            &Manifest::default(),
            &rules12,
            None,
            Some(&d),
            &new_files,
            &mut v5,
        );
        assert!(
            v5[0].message.contains("budget 10"),
            "min wins, was {}",
            v5[0].message
        );
    }

    #[test]
    fn a_dead_born_rule_owes_no_gate() {
        let tmp = std::env::temp_dir().join(format!("harna-born-dead-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("big.rs"), "x\n".repeat(50)).unwrap();
        let rel = tmp.to_string_lossy().to_string();
        let d = digest_of(vec![]);
        let mut rules = Rules::default();
        let mut r = budget_rule("dead-born", "born-size", 10, &[]);
        r.status = Some("deprecated".into());
        rules.rules.push(r);
        let mut v = Vec::new();
        check(
            Path::new(&rel),
            &Manifest::default(),
            &rules,
            None,
            Some(&d),
            &["big.rs".to_string()],
            &mut v,
        );
        assert!(v.is_empty(), "a dead rule cannot gate a birth");
    }
}
