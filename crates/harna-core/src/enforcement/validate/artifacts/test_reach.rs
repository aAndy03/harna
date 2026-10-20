//! `test-reach` (0.4.7): tests are code too, with a declared reach.
//!
//! Tests legitimately cross layers — an API integration test seeds the
//! store — so harna kept them out of layering. The household field test
//! showed the cost: `api` dev-depending on `store` passed unexamined, and
//! nothing said whether that crossing was meant. A manifest's
//! `[[test_kinds]]` declare it: where each kind lives (path globs) and
//! which layers it may reach beyond its home's own. The kind's files are
//! then held to layering with that reach added — the same check
//! production code meets, so the two never disagree — and the crates'
//! `[dev-dependencies]` to the same reach. No kinds declared: nothing runs.

use std::collections::BTreeSet;
use std::path::Path;

use super::Violation;
use crate::schema::model::manifest::{Manifest, TestKind};
use crate::search::digest::crates::CrateIndex;

pub fn check(root: &Path, manifest: &Manifest, out: &mut Vec<Violation>) {
    if manifest.test_kinds.is_empty() {
        return;
    }
    let files = crate::search::digest::respected_rust_files(root, 100_000);
    // Files grouped by the kinds covering them (0.4.9): a file two kinds
    // cover may reach the union of their reaches, and is checked once.
    let mut groups: std::collections::BTreeMap<Vec<&str>, Vec<(String, String)>> =
        Default::default();
    for f in &files {
        let kinds: Vec<&str> = manifest
            .test_kinds
            .iter()
            .filter(|k| k.covers(f))
            .map(|k| k.name.as_str())
            .collect();
        if kinds.is_empty() {
            continue;
        }
        if let Ok(src) = std::fs::read_to_string(root.join(f)) {
            groups.entry(kinds).or_default().push((f.clone(), src));
        }
    }
    for (names, srcs) in groups {
        let kinds: Vec<&TestKind> = manifest
            .test_kinds
            .iter()
            .filter(|k| names.contains(&k.name.as_str()))
            .collect();
        let widened = with_reach(manifest, &kinds);
        let mut found = Vec::new();
        crate::enforcement::validate::boundaries::boundary::check_homed(
            root, &widened, &srcs, true, &mut found,
        );
        let label = names.join("`/`");
        for mut v in found {
            if v.check == "layer-boundary" {
                v.check = "test-reach".into();
                v.severity = crate::enforcement::checks::severity_of("test-reach");
                v.message = format!("test kind `{label}`: {}", v.message);
                v.hint = format!(
                    "widen the kind's reach (`harna test-kind add {} --reach <layer>`) if the crossing is meant, or keep the test inside it",
                    names[0]
                );
            }
            out.push(v);
        }
    }
    dev_dependencies(root, manifest, &files, out);
}

/// The manifest as a kind's files see it: every layer may also reach the
/// kind's layers (`*` = all of them).
fn with_reach(manifest: &Manifest, kinds: &[&TestKind]) -> Manifest {
    let all: Vec<String> = manifest.layers.iter().map(|l| l.name.clone()).collect();
    let extra: Vec<String> = if kinds.iter().any(|k| k.reach.iter().any(|r| r == "*")) {
        all
    } else {
        kinds.iter().flat_map(|k| k.reach.iter().cloned()).collect()
    };
    let mut m = manifest.clone();
    for l in &mut m.layers {
        for e in &extra {
            if !l.depends_on.contains(e) && *e != l.name {
                l.depends_on.push(e.clone());
            }
        }
    }
    m
}

/// A crate's dev-dependencies may reach its own layers, their depends_on,
/// and the reach of every test kind whose paths cover files of the crate.
fn dev_dependencies(root: &Path, manifest: &Manifest, files: &[String], out: &mut Vec<Violation>) {
    let index = CrateIndex::load(root);
    let layers_of = |dir: &str| -> BTreeSet<String> {
        manifest
            .layers
            .iter()
            .filter(|l| {
                l.containers.iter().any(|c| {
                    !c.path.is_empty()
                        && index.locate_dir(&c.path).is_some_and(|(k, _)| k.dir == dir)
                })
            })
            .map(|l| l.name.clone())
            .collect()
    };
    // A kind covers a crate when it covers one of the crate's real files,
    // or (no test file yet) a conventional place for one (0.4.9).
    let probe = |dir: &str| -> Vec<String> {
        let at = |p: &str| {
            if dir == "." {
                p.to_string()
            } else {
                format!("{dir}/{p}")
            }
        };
        let mut v: Vec<String> = files
            .iter()
            .filter(|f| index.locate(f).is_some_and(|(k, _)| k.dir == dir))
            .cloned()
            .collect();
        v.extend([
            at("tests/probe.rs"),
            at("src/probe_tests.rs"),
            at("benches/probe.rs"),
        ]);
        v
    };
    for c in &index.crates {
        let own = layers_of(&c.dir);
        if own.is_empty() {
            continue;
        }
        let mut allowed: BTreeSet<String> = own.clone();
        for l in manifest.layers.iter().filter(|l| own.contains(&l.name)) {
            allowed.extend(l.depends_on.iter().cloned());
        }
        for kind in manifest
            .test_kinds
            .iter()
            .filter(|k| probe(&c.dir).iter().any(|p| k.covers(p)))
        {
            if kind.reach.iter().any(|r| r == "*") {
                allowed.extend(manifest.layers.iter().map(|l| l.name.clone()));
            } else {
                allowed.extend(kind.reach.iter().cloned());
            }
        }
        for target in index.cargo_dev_deps(root, &c.dir) {
            let theirs = layers_of(&target);
            let outside: Vec<&String> = theirs.iter().filter(|l| !allowed.contains(*l)).collect();
            if outside.is_empty() {
                continue;
            }
            let pkg = index
                .crates
                .iter()
                .find(|k| k.dir == target)
                .map(|k| k.package.clone())
                .unwrap_or(target.clone());
            out.push(Violation {
                check: "test-reach".into(),
                file: if c.dir == "." { "Cargo.toml".into() } else { format!("{}/Cargo.toml", c.dir) },
                line: 0,
                severity: crate::enforcement::checks::severity_of("test-reach"),
                message: format!(
                    "dev-dependency `{pkg}` reaches layer(s) {} that no test kind covering `{}` declares",
                    outside.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                    c.dir
                ),
                hint: "declare the crossing on a test kind (`harna test-kind add <kind> --path <glob> --reach <layer>`) or drop the dev-dependency".into(),
            });
        }
    }
}

#[cfg(test)]
mod tests;
