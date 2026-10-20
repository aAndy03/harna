//! Component `plugins-data` (0.5.0): installed plugins as data.
//!
//! A plugin is a versioned data pack for one framework or ecosystem
//! (gpui-kit 0.8.0, later TypeScript): its rules, its vocabulary, the
//! upstream versions it mirrors. Installed, it is vendored under
//! `.harna/plugins/<name>/` — reviewable, offline, versioned with the
//! code — and its rules join the project's own at load time, namespaced
//! `<plugin>/<id>` so they never collide with or overwrite `rules.toml`.
//! harna's core keeps no framework knowledge (rule `tools-not-prescriptions`);
//! the plugin carries it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::Store;
use crate::schema::model::rules::Rules;
use crate::schema::registers::terms::Terms;

/// `plugin.toml`: who the plugin is and what it mirrors.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The plugin's own version.
    #[serde(default)]
    pub version: String,
    /// Upstream packages it keeps parity with: `gpui-kit = "0.8.0"`. A
    /// Cargo package here is compared with `Cargo.lock` by the doctor.
    #[serde(default)]
    pub mirrors: BTreeMap<String, String>,
    /// Where its knowledge comes from (docs, guides), for the reader.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
}

/// `api.toml` (0.5.2): the public surface of the crate a plugin mirrors,
/// generated from its released source (`harna plugin api`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Api {
    /// The crate name as code spells it, e.g. `gpui_kit`.
    #[serde(rename = "crate")]
    pub crate_name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub modules: Vec<ApiModule>,
}

/// One module of the surface: `component::button`, its public names.
/// `open` = a glob re-export lives here; any name is trusted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiModule {
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub open: bool,
}

/// One installed plugin, read from `.harna/plugins/<name>/`.
#[derive(Debug, Clone)]
pub struct Installed {
    pub meta: PluginMeta,
    pub dir: PathBuf,
    pub rules: Rules,
    pub terms: Terms,
    pub api: Option<Api>,
}

impl Store {
    pub fn plugins_dir(&self) -> PathBuf {
        self.root.join("plugins")
    }

    /// Every installed plugin, by folder name. A folder whose files do not
    /// load is an error, never skipped: a broken pack must not silently
    /// stop enforcing.
    pub fn load_plugins(&self) -> anyhow::Result<Vec<Installed>> {
        let dir = self.plugins_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for d in dirs {
            let read = |f: &str| std::fs::read_to_string(d.join(f)).ok();
            let meta: PluginMeta = toml::from_str(
                &read("plugin.toml").with_context(|| format!("{}: no plugin.toml", d.display()))?,
            )
            .with_context(|| format!("{}/plugin.toml is not valid", d.display()))?;
            let rules: Rules = match read("rules.toml") {
                Some(t) => toml::from_str(&t)
                    .with_context(|| format!("{}/rules.toml is not valid", d.display()))?,
                None => Rules::default(),
            };
            let terms: Terms = match read("terms.toml") {
                Some(t) => toml::from_str(&t)
                    .with_context(|| format!("{}/terms.toml is not valid", d.display()))?,
                None => Terms::default(),
            };
            let api: Option<Api> = match read("api.toml") {
                Some(t) => Some(
                    toml::from_str(&t)
                        .with_context(|| format!("{}/api.toml is not valid", d.display()))?,
                ),
                None => None,
            };
            out.push(Installed {
                meta,
                dir: d,
                rules,
                terms,
                api,
            });
        }
        Ok(out)
    }

    /// The project's rules plus every installed plugin's, each plugin
    /// rule namespaced `<plugin>/<id>` and marked with its origin. The one
    /// read every check and briefing uses; edits still go to `rules.toml`
    /// (`load_rules` / `save_rules`) and never write a plugin's rules.
    pub fn load_effective_rules(&self) -> anyhow::Result<Rules> {
        let mut rules = self.load_rules()?;
        for p in self.load_plugins()? {
            for mut r in p.rules.rules {
                r.id = format!("{}/{}", p.meta.name, r.id);
                r.origin = format!("plugin {} {}", p.meta.name, p.meta.version);
                rules.rules.push(r);
            }
        }
        Ok(rules)
    }

    /// The glossary plus the plugins' vocabularies (a plugin's word never
    /// replaces the project's own).
    pub fn load_effective_terms(&self) -> anyhow::Result<Terms> {
        let mut terms = self.load_terms()?;
        for p in self.load_plugins()? {
            for t in p.terms.terms {
                if !terms
                    .terms
                    .iter()
                    .any(|x| x.word.eq_ignore_ascii_case(&t.word))
                {
                    terms.terms.push(t);
                }
            }
        }
        Ok(terms)
    }
}
