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

    /// Every contract on file, by container name (the doctor's audit).
    pub fn contract_names(&self) -> Vec<String> {
        std::fs::read_dir(self.contracts_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                e.file_name()
                    .to_str()
                    .and_then(|n| n.strip_suffix(".toml"))
                    .map(str::to_string)
            })
            .collect()
    }

    /// A container renamed: its contract follows (0.7.7: it was orphaned
    /// and the gate silently gone). Returns whether one moved.
    pub fn rename_contract(&self, old: &str, new: &str) -> anyhow::Result<bool> {
        let Some(mut c) = self.load_contract(old)? else {
            return Ok(false);
        };
        c.container = new.to_string();
        self.save_contract(&c)?;
        std::fs::remove_file(self.contracts_dir().join(format!("{old}.toml")))?;
        Ok(true)
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

/// A strict semver version, ordered (0.7.7): (major, minor, patch,
/// pre-release). A pre-release sorts before its release (`1.0.0-beta` <
/// `1.0.0`); anything else is not a version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Version(pub u64, pub u64, pub u64, pub Option<String>);

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.0, self.1, self.2)
            .cmp(&(o.0, o.1, o.2))
            .then_with(|| match (&self.3, &o.3) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

pub fn parse_version(v: &str) -> Option<Version> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
        Some(_) => return None,
        None => (v, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let n = |s: &str| s.parse::<u64>().ok();
    Some(Version(n(parts[0])?, n(parts[1])?, n(parts[2])?, pre))
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
