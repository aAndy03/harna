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
        let Ok(now) =
            crate::generation::structure::edit::contract::surface_now(manifest, d, &c.name)
        else {
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
    use crate::schema::model::manifest::{Container, Layer};
    use crate::search::digest::{Item, Module};

    fn digest(sig: &str, extra: bool) -> Digest {
        let mut inventory = vec![Item {
            vis: "pub".into(),
            kind: "fn".into(),
            name: "total".into(),
            text: sig.into(),
        }];
        if extra {
            inventory.push(Item {
                vis: "pub".into(),
                kind: "fn".into(),
                name: "average".into(),
                text: "pub fn average() -> Money".into(),
            });
        }
        Digest {
            modules: vec![Module {
                path: "crates/money/src/lib.rs".into(),
                lang: "rust".into(),
                inventory,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    /// 0.7.2: a frozen signature that changes is breaking; a new name is
    /// compatible (Info); a re-freeze at a new major accepts the break.
    #[test]
    fn contracts_tell_breaking_from_compatible() {
        let root = std::env::temp_dir().join(format!("harna-contract-{}", std::process::id()));
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
        let d1 = digest("pub fn total(a: &[Money]) -> Money", false);
        let c = crate::generation::structure::edit::contract::freeze(
            &m,
            &d1,
            "money",
            "1.0.0",
            None,
            "2026-10-20",
        )
        .unwrap();
        store.save_contract(&c).unwrap();
        let mut out = Vec::new();
        check(&store, &m, Some(&d1), &mut out);
        assert!(out.is_empty(), "{out:?}");
        let d2 = digest("pub fn total(a: &[Money], fx: Rate) -> Money", true);
        let mut out = Vec::new();
        check(&store, &m, Some(&d2), &mut out);
        let msgs: Vec<&str> = out.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(out.len(), 2, "{msgs:?}");
        assert!(msgs[0].contains("breaking change to contract `money` v1.0.0: `total` changed"));
        assert!(out[0].hint.contains("--version 2.0.0"));
        assert!(msgs[1].contains("gained 1 name(s) — compatible: average"));
        assert!(crate::generation::structure::edit::contract::freeze(
            &m,
            &d2,
            "money",
            "0.9.0",
            Some(&c),
            "x"
        )
        .is_err());
        let c2 = crate::generation::structure::edit::contract::freeze(
            &m,
            &d2,
            "money",
            "2.0.0",
            Some(&c),
            "x",
        )
        .unwrap();
        store.save_contract(&c2).unwrap();
        let mut out = Vec::new();
        check(&store, &m, Some(&d2), &mut out);
        assert!(out.is_empty(), "{out:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
