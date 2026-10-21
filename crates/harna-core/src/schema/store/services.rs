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
//!
//! Services talk through corridors (0.8.1): a queue, an event bus, an API —
//! the kind is the project's word. A corridor names the services that
//! send on it, the ones that receive, and each message with its shape: a
//! type in the code (`crates/events/src/lib.rs::PostingCreated`) or a
//! schema file (`schemas/posting.json`).
//!
//! Operational readiness (0.8.3): the project names the facets every
//! service must answer before a release (`health`, `backups`,
//! `migrations`, `rate-limits` — its own words), and each service
//! answers each one in a line: where it lives, or `n/a: <why>`. Preflight
//! holds a release while an answer is missing.

use std::collections::BTreeMap;
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corridors: Vec<Corridor>,
    /// Readiness facets every service must answer (0.8.3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub require: Vec<String>,
}

/// How services talk (0.8.1).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Corridor {
    pub name: String,
    /// event | queue | api | rpc | … (the project's word).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// Services that send on it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from: Vec<String>,
    /// Services that receive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
}

/// One message a corridor carries, with where its shape is declared.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub name: String,
    /// `<file>::<Type>` for a type in the code, or a schema file path.
    pub shape: String,
    /// Declared before its code exists (1.0, cost field test: a corridor
    /// could not be planned before the event type was written). Validate
    /// reminds while the shape is missing; preflight holds the release.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub planned: bool,
}

/// The declaration kinds a message shape can be: one list for the write
/// core, validate and preflight (1.0 review: `trait` was a shape in one
/// place and not in another, so a planned trait never counted as written).
pub const SHAPE_KINDS: &[&str] = &["struct", "enum", "type", "interface", "class", "trait"];

/// Whether `src` declares a shape named `ty`.
pub fn declares_shape(src: &str, ty: &str) -> bool {
    regex::Regex::new(&format!(
        r"\b(?:{})\s+{}\b",
        SHAPE_KINDS.join("|"),
        regex::escape(ty)
    ))
    .map(|re| re.is_match(src))
    .unwrap_or(true)
}

impl Message {
    /// Whether the shape is in the code: its file exists and, for
    /// `<file>::<Type>`, declares the type.
    pub fn in_code(&self, root: &std::path::Path) -> bool {
        let Some((file, ty)) = self.type_ref() else {
            // a schema file only has to exist (1.0 review: it was read)
            return root.join(&self.shape).is_file();
        };
        let Ok(src) = std::fs::read_to_string(root.join(file)) else {
            return false;
        };
        declares_shape(&src, ty)
    }

    /// `Name=shape` (CLI and MCP spelling); a bare shape names itself
    /// after its type, or its schema file's stem (0.8.4). The file part is
    /// normalized (`./`, backslashes).
    pub fn parse(spec: &str) -> anyhow::Result<Message> {
        let (name, shape) = match spec.split_once('=') {
            Some((n, s)) => (n.trim().to_string(), normalize_shape(s)),
            None => {
                let s = normalize_shape(spec);
                let n = match s.split_once("::") {
                    Some((_, ty)) => ty.rsplit("::").next().unwrap_or(ty).to_string(),
                    None => {
                        let file = s.rsplit('/').next().unwrap_or(&s);
                        file.split('.').next().unwrap_or(file).to_string()
                    }
                };
                (n, s)
            }
        };
        if name.is_empty() || shape.is_empty() || shape.ends_with("::") {
            anyhow::bail!("message `{spec}`: write Name=<file>::<Type> or Name=<schema file>");
        }
        Ok(Message {
            name,
            shape,
            planned: false,
        })
    }

    /// The shape split into (file, type) when it names a type in code; a
    /// nested path (`lib.rs::events::Posted`) names its last segment.
    pub fn type_ref(&self) -> Option<(&str, &str)> {
        let (file, path) = self.shape.split_once("::")?;
        Some((file, path.rsplit("::").next().unwrap_or(path)))
    }
}

fn normalize_shape(s: &str) -> String {
    let s = s.trim().replace('\\', "/");
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

impl Corridor {
    pub fn line(&self) -> String {
        let mut out = format!(
            "corridor {}{}: {} -> {}",
            self.name,
            if self.kind.is_empty() {
                String::new()
            } else {
                format!(" ({})", self.kind)
            },
            if self.from.is_empty() {
                "?".into()
            } else {
                self.from.join(", ")
            },
            if self.to.is_empty() {
                "?".into()
            } else {
                self.to.join(", ")
            },
        );
        if !self.messages.is_empty() {
            out.push_str(&format!(
                " carrying {}",
                self.messages
                    .iter()
                    .map(|m| if m.planned {
                        format!("{} (planned)", m.name)
                    } else {
                        m.name.clone()
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        out
    }
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
    /// Readiness facet -> its answer (`GET /healthz`, `n/a: stateless`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ready: BTreeMap<String, String>,
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

    /// One line per service, then per corridor, for briefings.
    pub fn lines(&self) -> Vec<String> {
        self.services
            .iter()
            .map(Service::line)
            .chain(self.corridors.iter().map(Corridor::line))
            .collect()
    }

    pub fn corridor(&self, name: &str) -> Option<&Corridor> {
        self.corridors.iter().find(|c| c.name == name)
    }

    pub fn is_empty(&self) -> bool {
        self.services.is_empty() && self.corridors.is_empty()
    }

    /// The lowest schema that holds this file (0.9.0): corridors (0.8.1)
    /// and readiness (0.8.3) arrived after 0.8.0 read services.toml, and a
    /// 0.8.0 harna would drop them on its next save.
    pub fn required_schema(&self) -> u32 {
        if self
            .corridors
            .iter()
            .flat_map(|c| &c.messages)
            .any(|m| m.planned)
        {
            6
        } else if !self.corridors.is_empty()
            || !self.require.is_empty()
            || self.services.iter().any(|s| !s.ready.is_empty())
        {
            5
        } else {
            crate::SCHEMA_BASELINE
        }
    }

    /// (corridor, message) for every planned message whose shape is not
    /// in the code yet: what a release still owes.
    pub fn planned_missing(&self, root: &std::path::Path) -> Vec<(String, String)> {
        self.corridors
            .iter()
            .flat_map(|c| c.messages.iter().map(move |m| (c, m)))
            .filter(|(_, m)| m.planned && !m.in_code(root))
            .map(|(c, m)| (c.name.clone(), m.name.clone()))
            .collect()
    }

    /// (service, facet) for every required facet a service has not
    /// answered (an empty answer counts as none).
    pub fn unready(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for s in &self.services {
            for f in &self.require {
                // a placeholder is no answer (0.8.4, books field test)
                if s.ready.get(f).is_none_or(|a| is_placeholder(a)) {
                    out.push((s.name.clone(), f.clone()));
                }
            }
        }
        out
    }
}

/// The manifest with each service's owners standing in for containers
/// that name none (0.8.4, books field test: a service's `--owners` were
/// ignored by `harna owners` and CODEOWNERS). Container owners still win;
/// the layer's answer only when neither speaks.
pub fn with_service_owners(
    manifest: &crate::schema::model::manifest::Manifest,
    services: &Services,
) -> crate::schema::model::manifest::Manifest {
    let mut m = manifest.clone();
    for c in m.layers.iter_mut().flat_map(|l| l.containers.iter_mut()) {
        if c.owners.is_empty() {
            if let Some(s) = services.service_of(&c.name) {
                c.owners = s.owners.clone();
            }
        }
    }
    m
}

/// `with_service_owners` with the services read from the repo at `root`.
pub fn owners_view(
    root: &std::path::Path,
    manifest: &crate::schema::model::manifest::Manifest,
) -> crate::schema::model::manifest::Manifest {
    match Store::open(root).and_then(|s| s.load_services()) {
        Ok(s) if !s.services.is_empty() => with_service_owners(manifest, &s),
        _ => manifest.clone(),
    }
}

/// An answer that answers nothing (0.9.2, field test: `TODO: later`,
/// `FIXME`, `???`, `none` and `x` all passed preflight): empty, a bare
/// `n/a`, a TODO/TBD/FIXME/XXX note, only punctuation, or one character.
pub fn is_placeholder(answer: &str) -> bool {
    let a = answer.trim().to_ascii_lowercase();
    let word: String = a
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    a.is_empty()
        || a.chars().count() <= 1
        || a.chars().all(|c| !c.is_alphanumeric())
        || matches!(
            a.as_str(),
            "none" | "n/a" | "n/a:" | "na" | "-" | "wip" | "later"
        )
        || (a.starts_with("n/a") && a[3..].trim_start_matches(':').trim().is_empty())
        || matches!(word.as_str(), "todo" | "tbd" | "fixme" | "xxx" | "tba")
}

/// A resource as one spelling: no spaces around the `:` (`db: ledger` is
/// `db:ledger`, 0.9.2), case kept.
pub fn normalize_resource(r: &str) -> String {
    match r.split_once(':') {
        Some((k, n)) => format!("{}:{}", k.trim(), n.trim()),
        None => r.trim().to_string(),
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
        super::check_schema_version(s.schema_version).with_context(|| format!(".harna/{FILE}"))?;
        Ok(s)
    }

    pub fn save_services(&self, s: &Services) -> anyhow::Result<()> {
        self.guard_unknown(&self.services_path(), self.load_services())?;
        std::fs::create_dir_all(&self.root)?;
        let mut s = s.clone();
        s.schema_version = s.required_schema();
        super::atomic::write(self.services_path(), toml::to_string_pretty(&s)?)
            .with_context(|| format!("writing .harna/{FILE}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0.8.3: every required facet a service has not answered (or answered
    /// blank) is unready; `n/a: why` is an answer.
    #[test]
    fn readiness_lists_what_is_unanswered() {
        let mut s = Services {
            require: vec!["health".into(), "backups".into()],
            services: vec![
                Service {
                    name: "api".into(),
                    ready: [
                        ("health".to_string(), "GET /healthz".to_string()),
                        ("backups".to_string(), "n/a: stateless".to_string()),
                    ]
                    .into(),
                    ..Default::default()
                },
                Service {
                    name: "worker".into(),
                    ready: [("health".to_string(), " ".to_string())].into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            s.unready(),
            vec![
                ("worker".to_string(), "health".to_string()),
                ("worker".to_string(), "backups".to_string())
            ]
        );
        s.require.clear();
        assert!(s.unready().is_empty(), "nothing required, nothing held");
    }
}

#[cfg(test)]
mod placeholder_tests {
    /// 0.9.2 (field test): notes to self are not answers; real ones are.
    #[test]
    fn placeholders_answer_nothing() {
        for p in [
            "",
            "TODO",
            "todo: later",
            "TODO: wire later",
            "FIXME",
            "???",
            "none",
            "x",
            "n/a",
            "n/a:",
            "-",
        ] {
            assert!(super::is_placeholder(p), "{p:?} is a placeholder");
        }
        for a in [
            "GET /healthz",
            "n/a: stateless",
            "crates/api/migrations",
            "nightly pg_dump to S3",
        ] {
            assert!(!super::is_placeholder(a), "{a:?} is an answer");
        }
        assert_eq!(super::normalize_resource(" db : ledger "), "db:ledger");
    }
}
