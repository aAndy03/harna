//! Component `affected` (0.7.5): CI that runs what a change can break.
//!
//! `harna affected [--base main]` takes the files changed against a base
//! ref (or the working tree), the containers holding them, and the
//! containers whose modules import those files (one ring, read from the
//! map); then names the crates and TS folders to test. `--json` prints it
//! as a CI matrix. A change outside every container widens to everything,
//! honestly — unknown blast radius is not a small one.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use crate::schema::model::manifest::Manifest;
use crate::search::digest::Digest;

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct Affected {
    pub changed: Vec<String>,
    /// Containers holding a changed file.
    pub direct: Vec<String>,
    /// Containers importing a changed file (one ring).
    pub importers: Vec<String>,
    /// Cargo packages to test (`cargo test -p <name>`).
    pub crates: Vec<String>,
    /// Packages depending on those, transitively, by Cargo.toml (0.7.7:
    /// a break compiles through every dependent, not one import ring).
    pub dependent_crates: Vec<String>,
    /// Services running an affected container (0.8.4): what to redeploy.
    #[serde(default)]
    pub services: Vec<String>,
    /// TS/JS container folders to test (0.7.7).
    pub folders: Vec<String>,
    /// Something whose reach cannot be read: test everything.
    pub everything: bool,
    /// Why everything (0.7.7: say it).
    pub reasons: Vec<String>,
}

/// Files changed against `base` (`<base>...HEAD`, plus the working tree);
/// the working tree alone when no base is given.
pub fn changed_files(root: &Path, base: Option<&str>) -> anyhow::Result<Vec<String>> {
    let mut files: BTreeSet<String> = crate::enforcement::validate::git::changed_files(root)?
        .into_iter()
        .collect();
    if let Some(b) = base {
        let out = std::process::Command::new("git")
            .stdin(std::process::Stdio::null())
            // --relative: paths from the harna root, which may be a git
            // subfolder (0.9.2: two path styles mixed in owners/affected)
            .args(["diff", "--relative", "--name-only", &format!("{b}...HEAD")])
            .current_dir(root)
            .output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git diff against `{b}` failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        files.extend(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.trim().replace('\\', "/")),
        );
        // deleted and renamed-away paths too (0.7.7): their importers break
        let status = std::process::Command::new("git")
            .stdin(std::process::Stdio::null())
            .args([
                "diff",
                "--relative",
                "--name-status",
                "-M",
                &format!("{b}...HEAD"),
            ])
            .current_dir(root)
            .output()?;
        for line in String::from_utf8_lossy(&status.stdout).lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 && (parts[0].starts_with('D') || parts[0].starts_with('R')) {
                files.insert(parts[1].replace('\\', "/"));
            }
        }
    }
    Ok(files.into_iter().collect())
}

/// Name the services whose containers the change reaches (directly or by
/// import); a change reaching shared code names every service linking it.
pub fn with_services(
    mut a: Affected,
    services: &crate::schema::store::services::Services,
    root: &Path,
    manifest: &Manifest,
) -> Affected {
    let mut hit: BTreeSet<String> = BTreeSet::new();
    for c in a.direct.iter().chain(&a.importers) {
        if let Some(s) = services.service_of(c) {
            hit.insert(s.name.clone());
        }
    }
    // a service whose crate depends on a changed crate, however far down,
    // ships the change too (0.9.2, codex field test: `cli` was missed)
    let index = crate::search::digest::crates::CrateIndex::load(root);
    for s in &services.services {
        let reached = s.containers.iter().any(|c| {
            manifest
                .layers
                .iter()
                .flat_map(|l| &l.containers)
                .find(|x| &x.name == c)
                .and_then(|x| index.locate_dir(&x.path))
                .is_some_and(|(k, _)| {
                    a.crates.contains(&k.package) || a.dependent_crates.contains(&k.package)
                })
        });
        if reached {
            hit.insert(s.name.clone());
        }
    }
    a.services = hit.into_iter().collect();
    a
}

/// A warning when the working tree changed files the map does not
/// describe yet (0.7.7): what reads the map reads it as it was.
pub fn map_note(root: &Path, digest: &Digest) -> Option<String> {
    let changed = crate::enforcement::validate::git::changed_files(root).ok()?;
    let stale = crate::search::digest::stale_paths(root, digest, &changed);
    if stale.is_empty() {
        return None;
    }
    let shown: Vec<&str> = stale.iter().take(3).map(String::as_str).collect();
    Some(format!(
        "the map is stale for {} changed file(s) ({}{}) — run `harna scan` first",
        stale.len(),
        shown.join(", "),
        if stale.len() > 3 { ", …" } else { "" }
    ))
}

pub fn compute(root: &Path, manifest: &Manifest, digest: &Digest, changed: &[String]) -> Affected {
    let home = |path: &str| -> Option<String> {
        manifest
            .layers
            .iter()
            .flat_map(|l| &l.containers)
            .filter(|c| crate::schema::model::scope::path_in_container(path, &c.path))
            .max_by_key(|c| c.path.len())
            .or_else(|| {
                // a crate's Cargo.toml or tests/ beside its `src` container
                manifest
                    .layers
                    .iter()
                    .flat_map(|l| &l.containers)
                    .find(|c| {
                        crate::schema::model::manifest::owner_root(&c.path).is_some_and(|r| {
                            crate::schema::model::scope::path_in_container(path, &r)
                        })
                    })
            })
            .map(|c| c.name.clone())
    };
    let mut a = Affected {
        changed: changed.to_vec(),
        ..Default::default()
    };
    let mut direct = BTreeSet::new();
    let name = |f: &str| f.rsplit('/').next().unwrap_or(f).to_string();
    for f in changed {
        // .harna/ and docs move no code: they need no tests
        if f.starts_with(".harna/") || f.ends_with(".md") {
            continue;
        }
        // dependency manifests and lockfiles reach everything (0.7.7)
        let n = name(f);
        if matches!(
            n.as_str(),
            "Cargo.lock"
                | "package.json"
                | "package-lock.json"
                | "pnpm-lock.yaml"
                | "yarn.lock"
                | "bun.lockb"
                | "deno.json"
                | "deno.lock"
        ) || (n.starts_with("tsconfig") && n.ends_with(".json"))
            || (f == "Cargo.toml")
        {
            a.everything = true;
            a.reasons.push(format!(
                "{f}: dependencies or build settings for every crate/package"
            ));
            continue;
        }
        // a source file gone (deleted, renamed away): who imported it is
        // no longer in the map — honest about the unknown
        let source = f.ends_with(".rs") || crate::search::digest::ts::is_source(f);
        if source && !root.join(f).exists() {
            a.everything = true;
            a.reasons
                .push(format!("{f} is gone: its importers are not in the new map"));
        }
        match home(f) {
            Some(c) => {
                direct.insert(c);
            }
            None if f.ends_with(".rs")
                || crate::search::digest::ts::is_source(f)
                || f.ends_with("Cargo.toml") =>
            {
                a.everything = true;
                a.reasons.push(format!("{f} sits outside every container"));
            }
            None => {}
        }
    }
    // one ring of importers, through the shared edge reading
    let crate_names = crate::search::digest::workspace_crate_names(root);
    let name_dirs = crate::search::digest::crate_name_dirs(
        root,
        &digest
            .modules
            .iter()
            .filter_map(|m| m.crate_of().0)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>(),
    );
    let targets: Vec<&crate::search::digest::Module> = digest
        .modules
        .iter()
        .filter(|m| changed.contains(&m.path))
        .collect();
    let mut importers = BTreeSet::new();
    let roots: Vec<Option<&crate::search::digest::Module>> = targets
        .iter()
        .map(|t| crate::search::digest::crate_root_of(&digest.modules, t))
        .collect();
    for m in &digest.modules {
        if targets.iter().zip(&roots).any(|(t, root)| {
            t.path != m.path
                && m.deps.iter().any(|d| {
                    crate::search::digest::dep_reaches(m, d, t, &crate_names, &name_dirs)
                        || root.is_some_and(|r| {
                            crate::search::digest::reaches_through_root(
                                m,
                                d,
                                r,
                                t,
                                &crate_names,
                                &name_dirs,
                            )
                        })
                })
        }) {
            if let Some(c) = home(&m.path) {
                if !direct.contains(&c) {
                    importers.insert(c);
                }
            }
        }
    }
    // the crates holding any affected container
    let index = crate::search::digest::crates::CrateIndex::load(root);
    let mut crates = BTreeSet::new();
    for c in manifest
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .filter(|c| direct.contains(&c.name) || importers.contains(&c.name))
    {
        if let Some((k, _)) = index.locate_dir(&c.path) {
            crates.insert(k.package.clone());
        }
    }
    // TS/JS folders among the affected containers
    a.folders = manifest
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .filter(|c| direct.contains(&c.name) || importers.contains(&c.name))
        .filter(|c| {
            digest.modules.iter().any(|m| {
                (m.lang == "typescript" || m.lang == "javascript")
                    && crate::schema::model::scope::path_in_container(&m.path, &c.path)
            })
        })
        .map(|c| c.path.clone())
        .collect();
    // Cargo reverse dependencies, to the fixpoint (normal and dev)
    let deps_of: Vec<(String, Vec<String>)> = index
        .crates
        .iter()
        .map(|k| {
            let mut d = index.cargo_deps(root, &k.dir);
            d.extend(index.cargo_dev_deps(root, &k.dir));
            (k.dir.clone(), d)
        })
        .collect();
    let package_of = |dir: &str| {
        index
            .crates
            .iter()
            .find(|k| k.dir == dir)
            .map(|k| k.package.clone())
    };
    let mut reached: BTreeSet<String> = index
        .crates
        .iter()
        .filter(|k| crates.contains(&k.package))
        .map(|k| k.dir.clone())
        .collect();
    loop {
        let more: Vec<String> = deps_of
            .iter()
            .filter(|(dir, d)| !reached.contains(dir) && d.iter().any(|x| reached.contains(x)))
            .map(|(dir, _)| dir.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        reached.extend(more);
    }
    a.dependent_crates = reached
        .iter()
        .filter_map(|d| package_of(d))
        .filter(|p| !crates.contains(p))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    a.direct = direct.into_iter().collect();
    a.importers = importers.into_iter().collect();
    a.crates = crates.into_iter().collect();
    a
}

impl Affected {
    pub fn render(&self) -> String {
        if self.changed.is_empty() {
            return "nothing changed — nothing to run\n".into();
        }
        let mut out = format!("{} changed file(s)\n", self.changed.len());
        if self.everything {
            out.push_str("test everything:\n");
            for r in &self.reasons {
                out.push_str(&format!("  {r}\n"));
            }
        }
        out.push_str(&format!("containers changed: {}\n", list(&self.direct)));
        out.push_str(&format!(
            "containers importing them: {}\n",
            list(&self.importers)
        ));
        if !self.crates.is_empty() {
            out.push_str(&format!(
                "run: cargo test {}\n",
                self.crates
                    .iter()
                    .map(|c| format!("-p {c}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        if !self.services.is_empty() {
            out.push_str(&format!(
                "services to redeploy: {}\n",
                self.services.join(", ")
            ));
        }
        if !self.dependent_crates.is_empty() {
            out.push_str(&format!(
                "crates depending on them (Cargo, transitive): {}
",
                self.dependent_crates.join(", ")
            ));
        }
        if !self.folders.is_empty() {
            out.push_str(&format!(
                "TS/JS folders to test: {}\n",
                self.folders.join(", ")
            ));
        }
        out
    }
}

fn list(v: &[String]) -> String {
    if v.is_empty() {
        "none".into()
    } else {
        v.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};
    use crate::search::digest::Module;

    /// 0.7.5: a changed module's container, the containers importing it
    /// (one ring), their crates; a source file nobody holds widens to all.
    #[test]
    fn a_change_routes_to_its_containers_and_importers() {
        let root = std::env::temp_dir().join(format!("harna-affected-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        w("crates/money/Cargo.toml", "[package]\nname = \"money\"\n");
        w("crates/ledger/Cargo.toml", "[package]\nname = \"ledger\"\n");
        w("crates/ui/Cargo.toml", "[package]\nname = \"ui\"\n");
        for f in [
            "crates/money/src/lib.ts",
            "crates/ledger/src/post.ts",
            "crates/ui/src/view.ts",
        ] {
            w(f, "export const x = 1;\n");
        }
        let c = |name: &str, path: &str| Container {
            name: name.into(),
            path: path.into(),
            ..Default::default()
        };
        let m = Manifest {
            layers: vec![Layer {
                name: "all".into(),
                containers: vec![
                    c("money", "crates/money/src"),
                    c("ledger", "crates/ledger/src"),
                    c("ui", "crates/ui/src"),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let module = |path: &str, deps: &[&str]| Module {
            path: path.into(),
            lang: "typescript".into(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let d = Digest {
            modules: vec![
                module("crates/money/src/lib.ts", &[]),
                module(
                    "crates/ledger/src/post.ts",
                    &["file:crates/money/src/lib.ts"],
                ),
                module("crates/ui/src/view.ts", &[]),
            ],
            ..Default::default()
        };
        let a = compute(
            &root,
            &m,
            &d,
            &["crates/money/src/lib.ts".into(), "README.md".into()],
        );
        assert_eq!(a.direct, vec!["money"]);
        assert_eq!(a.importers, vec!["ledger"]);
        assert_eq!(a.crates, vec!["ledger", "money"]);
        assert!(!a.everything);
        let b = compute(&root, &m, &d, &["tools/gen.rs".into()]);
        assert!(b.everything);
        assert_eq!(a.folders, vec!["crates/money/src", "crates/ledger/src"]);
        let lock = compute(&root, &m, &d, &["Cargo.lock".into()]);
        assert!(lock.everything && lock.reasons[0].contains("Cargo.lock"));
        let gone = compute(&root, &m, &d, &["crates/money/src/old.ts".into()]);
        assert!(gone.everything && gone.reasons[0].contains("is gone"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
