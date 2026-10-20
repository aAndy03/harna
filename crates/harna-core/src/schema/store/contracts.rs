//! Component `contracts-data` (0.7.2): a container's surface, frozen at a
//! version. `.harna/contracts/<container>.toml` holds the version and each
//! provided name with the signature it had then; validate compares the
//! code against it (check `contract`). A contract is the promise other
//! containers — and other teams (0.7.0 owners) — build against.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contract {
    pub container: String,
    /// The contract's version, semver (`1.2.0`).
    pub version: String,
    /// When it was frozen (ISO date).
    #[serde(default)]
    pub frozen: String,
    /// Provided name -> its signature (declaration line) at that version.
    #[serde(default)]
    pub items: BTreeMap<String, String>,
}

impl Store {
    pub fn contracts_dir(&self) -> PathBuf {
        self.root.join("contracts")
    }

    pub fn load_contract(&self, container: &str) -> anyhow::Result<Option<Contract>> {
        let path = self.contracts_dir().join(format!("{container}.toml"));
        if !path.is_file() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;
        toml::from_str(&raw)
            .map(Some)
            .with_context(|| format!("{} is not valid", path.display()))
    }

    pub fn save_contract(&self, c: &Contract) -> anyhow::Result<()> {
        if !crate::schema::store::plugins::valid_plugin_name(&c.container.to_lowercase())
            && c.container.contains(['/', '\\', ':', '.'])
        {
            anyhow::bail!("`{}` is not a container name", c.container);
        }
        std::fs::create_dir_all(self.contracts_dir())?;
        crate::schema::store::atomic::write(
            self.contracts_dir().join(format!("{}.toml", c.container)),
            toml::to_string_pretty(c)?,
        )
    }
}

/// `major.minor.patch` as numbers (missing parts are 0).
pub fn semver(v: &str) -> (u64, u64, u64) {
    let mut p = v
        .trim_start_matches('v')
        .split('.')
        .map(|x| x.trim().parse::<u64>().unwrap_or(0));
    (
        p.next().unwrap_or(0),
        p.next().unwrap_or(0),
        p.next().unwrap_or(0),
    )
}
