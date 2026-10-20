//! Component `services-data` (0.8.0): which services exist and what each
//! owns, as data.
//!
//! A service is a unit that runs on its own: a binary, a worker, an API.
//! `.harna/services.toml` names each one, the containers it runs (the code
//! only it ships), the resources it alone writes (a database, a queue, a
//! bucket — free text `kind:name`, the project's own words) and, when it
//! has one, the folder of its deploy unit. A container no service names is
//! shared code, linked into whichever services import it. harna stores
//! and checks this; it never decides how a product splits into services
//! (rule `tools-not-prescriptions`).

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::Store;

const FILE: &str = "services.toml";

fn current() -> u32 {
    crate::SCHEMA_BASELINE
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Services {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The containers only this service runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub containers: Vec<String>,
    /// Resources only this service writes (`db:ledger`, `queue:postings`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owns: Vec<String>,
    /// Who answers for it; empty = its containers' owners.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owners: Vec<String>,
    /// The folder of its deploy unit (a crate, a package), when it has one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub runs: String,
}

impl Services {
    pub fn get(&self, name: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.name == name)
    }

    /// The service running `container`, if one names it.
    pub fn service_of(&self, container: &str) -> Option<&Service> {
        self.services
            .iter()
            .find(|s| s.containers.iter().any(|c| c == container))
    }

    /// The service owning `resource`, if one declares it.
    pub fn owner_of(&self, resource: &str) -> Option<&Service> {
        self.services
            .iter()
            .find(|s| s.owns.iter().any(|r| r == resource))
    }

    /// One line per service, for briefings.
    pub fn lines(&self) -> Vec<String> {
        self.services.iter().map(Service::line).collect()
    }
}

impl Service {
    pub fn line(&self) -> String {
        let mut out = format!("service {}", self.name);
        if !self.containers.is_empty() {
            out.push_str(&format!(" [{}]", self.containers.join(", ")));
        }
        if !self.owns.is_empty() {
            out.push_str(&format!(" owns {}", self.owns.join(", ")));
        }
        if !self.description.is_empty() {
            out.push_str(&format!(" — {}", self.description));
        }
        out
    }
}

impl Store {
    pub fn services_path(&self) -> PathBuf {
        self.root.join(FILE)
    }

    /// The services; a missing file is none declared.
    pub fn load_services(&self) -> anyhow::Result<Services> {
        let path = self.services_path();
        if !path.is_file() {
            return Ok(Services::default());
        }
        let raw = std::fs::read_to_string(&path)?;
        let s: Services =
            toml::from_str(&raw).with_context(|| format!(".harna/{FILE} is not valid"))?;
        if s.schema_version > crate::SCHEMA_VERSION {
            anyhow::bail!(
                ".harna/{FILE} is schema v{} — this harna reads up to v{}: upgrade harna",
                s.schema_version,
                crate::SCHEMA_VERSION
            );
        }
        Ok(s)
    }

    pub fn save_services(&self, s: &Services) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let mut s = s.clone();
        s.schema_version = crate::SCHEMA_BASELINE;
        super::atomic::write(self.services_path(), toml::to_string_pretty(&s)?)
            .with_context(|| format!("writing .harna/{FILE}"))
    }
}
