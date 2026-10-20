//! Component `contract-freeze` (0.7.2): freeze a container's surface as a
//! versioned contract. The surface is its declared `provides` (every
//! public item of the container when none is declared); each name keeps
//! the declaration line the map has for it. Re-freezing at a higher
//! version is how a breaking change is accepted.

use std::collections::BTreeMap;

use crate::schema::model::manifest::Manifest;
use crate::schema::store::contracts::{semver, Contract};
use crate::search::digest::Digest;

/// The container's current surface: provided name -> signature.
pub fn surface_now(
    m: &Manifest,
    d: &Digest,
    container: &str,
) -> anyhow::Result<BTreeMap<String, String>> {
    let c = m
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .find(|c| c.name == container)
        .ok_or_else(|| anyhow::anyhow!("no container `{container}`"))?;
    if c.path.is_empty() {
        anyhow::bail!("container `{container}` has no path — a contract is frozen from its code");
    }
    let mut provided: Vec<String> = c.provides.clone();
    for comp in &c.components {
        provided.extend(comp.provides.iter().cloned());
    }
    let mut out = BTreeMap::new();
    for md in d
        .modules
        .iter()
        .filter(|md| crate::schema::model::scope::path_in_container(&md.path, &c.path))
    {
        for item in md.inventory.iter().filter(|i| i.vis == "pub") {
            if provided.is_empty() || provided.contains(&item.name) {
                out.entry(item.name.clone())
                    .or_insert_with(|| item.text.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
    }
    Ok(out)
}

/// A new contract for `container` at `version` (must not go backwards).
pub fn freeze(
    m: &Manifest,
    d: &Digest,
    container: &str,
    version: &str,
    previous: Option<&Contract>,
    today: &str,
) -> anyhow::Result<Contract> {
    if let Some(p) = previous {
        if semver(version) < semver(&p.version) {
            anyhow::bail!(
                "contract `{container}` is at {}; {version} would go backwards",
                p.version
            );
        }
    }
    let items = surface_now(m, d, container)?;
    if items.is_empty() {
        anyhow::bail!(
            "container `{container}` exposes nothing yet (run `harna scan`, or declare provides)"
        );
    }
    Ok(Contract {
        container: container.into(),
        version: version.into(),
        frozen: today.into(),
        items,
    })
}
