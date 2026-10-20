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
    /// A changed file outside every container: test everything.
    pub everything: bool,
}

/// Files changed against `base` (`<base>...HEAD`, plus the working tree);
/// the working tree alone when no base is given.
pub fn changed_files(root: &Path, base: Option<&str>) -> anyhow::Result<Vec<String>> {
    let mut files: BTreeSet<String> = crate::enforcement::validate::git::changed_files(root)?
        .into_iter()
        .collect();
    if let Some(b) = base {
        let out = std::process::Command::new("git")
            .args(["diff", "--name-only", &format!("{b}...HEAD")])
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
    }
    Ok(files.into_iter().collect())
}

pub fn compute(root: &Path, manifest: &Manifest, digest: &Digest, changed: &[String]) -> Affected {
    let home = |path: &str| -> Option<String> {
        manifest
            .layers
            .iter()
            .flat_map(|l| &l.containers)
            .filter(|c| crate::schema::model::scope::path_in_container(path, &c.path))
            .max_by_key(|c| c.path.len())
            .map(|c| c.name.clone())
    };
    let mut a = Affected {
        changed: changed.to_vec(),
        ..Default::default()
    };
    let mut direct = BTreeSet::new();
    for f in changed {
        // .harna/ and docs move no code: they need no tests
        if f.starts_with(".harna/") || f.ends_with(".md") {
            continue;
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
    for m in &digest.modules {
        if targets.iter().any(|t| {
            t.path != m.path
                && m.deps
                    .iter()
                    .any(|d| crate::search::digest::dep_reaches(m, d, t, &crate_names, &name_dirs))
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
            out.push_str("a changed source file sits outside every container — test everything\n");
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
        let _ = std::fs::remove_dir_all(&root);
    }
}
