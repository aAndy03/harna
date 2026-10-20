//! `contract` (0.7.2): a frozen contract against today's code.
//!
//! For each container with a contract (`.harna/contracts/<c>.toml`):
//! - a frozen name that is gone, or whose signature changed, is BREAKING —
//!   an error until the contract is re-frozen at a new major version
//!   (`harna contract freeze <c> --version 2.0.0`) or the change undone;
//! - a provided name the contract does not hold is COMPATIBLE — named
//!   (Info) so a minor version records it.
//!
//! Compatible vs breaking is the question parity could not answer.

use super::Violation;
use crate::schema::model::manifest::Manifest;
use crate::schema::store::Store;
use crate::search::digest::Digest;

pub fn check(
    store: &Store,
    manifest: &Manifest,
    digest: Option<&Digest>,
    out: &mut Vec<Violation>,
) {
    let Some(d) = digest else {
        return;
    };
    for c in manifest.layers.iter().flat_map(|l| &l.containers) {
        let Ok(Some(contract)) = store.load_contract(&c.name) else {
            continue;
        };
        let Ok(now) = crate::generation::structure::edit::contract::surface_now(
            &store.repo_root(),
            manifest,
            d,
            &c.name,
        ) else {
            continue;
        };
        let file = format!(".harna/contracts/{}.toml", c.name);
        for (name, was) in &contract.items {
            let problem = match now.get(name) {
                None => format!("`{name}` is gone"),
                Some(is) if is != was => format!("`{name}` changed: `{was}` -> `{is}`"),
                Some(_) => continue,
            };
            out.push(Violation {
                check: "contract".into(),
                file: file.clone(),
                line: 0,
                severity: crate::enforcement::checks::severity_of("contract"),
                message: format!(
                    "breaking change to contract `{}` v{}: {problem}",
                    c.name, contract.version
                ),
                hint: format!(
                    "keep it compatible, or accept the break: harna contract freeze {} --version {}.0.0 (and tell its importers: harna dependents)",
                    c.name,
                    crate::schema::store::contracts::semver(&contract.version).0 + 1
                ),
            });
        }
        let added: Vec<&String> = now
            .keys()
            .filter(|k| !contract.items.contains_key(*k))
            .collect();
        if !added.is_empty() {
            let (maj, min, _) = crate::schema::store::contracts::semver(&contract.version);
            out.push(Violation {
                check: "contract".into(),
                file,
                line: 0,
                severity: crate::enforcement::violation::Severity::Info,
                message: format!(
                    "contract `{}` v{} gained {} name(s) — compatible: {}",
                    c.name,
                    contract.version,
                    added.len(),
                    added
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                hint: format!(
                    "record it: harna contract freeze {} --version {maj}.{}.0",
                    c.name,
                    min + 1
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::structure::edit::contract::freeze;
    use crate::schema::model::manifest::{Container, Layer};
    use crate::search::digest::Module;

    fn setup(tag: &str) -> (std::path::PathBuf, Store, Manifest, Digest) {
        let root =
            std::env::temp_dir().join(format!("harna-contract-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = Store::open(&root).unwrap();
        store.init().unwrap();
        let m = Manifest {
            layers: vec![Layer {
                name: "core".into(),
                containers: vec![Container {
                    name: "money".into(),
                    path: "crates/money/src".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let d = Digest {
            modules: vec![Module {
                path: "crates/money/src/lib.rs".into(),
                lang: "rust".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        (root, store, m, d)
    }

    fn write(root: &std::path::Path, src: &str) {
        let p = root.join("crates/money/src/lib.rs");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    /// 0.7.2/0.7.7: a re-signed name is breaking; a new name compatible;
    /// a body edit, a rustfmt wrap or a const value is no change; two
    /// `new`s are told apart; a break needs the next major to be frozen.
    #[test]
    fn contracts_tell_breaking_from_compatible() {
        let (root, store, m, d) = setup("a");
        write(
            &root,
            concat!(
                "pub const LIMIT: u32 = 10;\n",
                "pub fn total(a: &[Money]) -> Money { a.iter().sum() }\n",
                "pub struct A;\nimpl A {\n    pub fn new(x: u32) -> Self { A }\n}\n",
                "pub struct B;\nimpl B {\n    pub fn new(x: u32) -> Self { B }\n}\n",
            ),
        );
        let c = freeze(&root, &m, &d, "money", "1.0.0", None, "2026-10-20").unwrap();
        assert!(
            c.items.contains_key("A::new") && c.items.contains_key("B::new"),
            "{:?}",
            c.items
        );
        store.save_contract(&c).unwrap();
        let run = || {
            let mut out = Vec::new();
            check(&store, &m, Some(&d), &mut out);
            out
        };
        assert!(run().is_empty());
        // a body edit, a wrap, a const value, spacing: no change
        write(
            &root,
            concat!(
                "pub const LIMIT: u32 = 11;\n",
                "pub fn total(\n    a : &[Money],\n) -> Money {\n    a.iter().copied().sum()\n}\n",
                "pub struct A;\nimpl A {\n    pub fn new(x: u32) -> Self { A }\n}\n",
                "pub struct B;\nimpl B {\n    pub fn new(x: u32) -> Self { B }\n}\n",
            ),
        );
        assert!(run().is_empty(), "{:?}", run());
        // B::new re-signed on its own; a new fn added
        write(
            &root,
            concat!(
                "pub const LIMIT: u32 = 11;\n",
                "pub fn total(a: &[Money]) -> Money { a.iter().sum() }\n",
                "pub fn average(a: &[Money]) -> Money { todo!() }\n",
                "pub struct A;\nimpl A {\n    pub fn new(x: u32) -> Self { A }\n}\n",
                "pub struct B;\nimpl B {\n    pub fn new(x: u64) -> Self { B }\n}\n",
            ),
        );
        let out = run();
        let msgs: Vec<&str> = out.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(out.len(), 2, "{msgs:?}");
        assert!(msgs[0].contains("`B::new` changed"), "{msgs:?}");
        assert!(msgs[1].contains("compatible: average"), "{msgs:?}");
        // the break cannot be silenced at the same or a minor version
        assert!(freeze(&root, &m, &d, "money", "1.0.0", Some(&c), "x").is_err());
        assert!(freeze(&root, &m, &d, "money", "1.1.0", Some(&c), "x").is_err());
        assert!(freeze(&root, &m, &d, "money", "banana", Some(&c), "x").is_err());
        assert!(freeze(&root, &m, &d, "money", "2.0.0-beta.1", Some(&c), "x").is_ok());
        let c2 = freeze(&root, &m, &d, "money", "2.0.0", Some(&c), "x").unwrap();
        store.save_contract(&c2).unwrap();
        assert!(run().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
