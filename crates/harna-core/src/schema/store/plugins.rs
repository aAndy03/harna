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
    /// Named capabilities (0.5.4): the framework's tokens one container
    /// may own (`raw-input = ["on_mouse_down(", …]`), named in the
    /// manifest as `<plugin>:<capability>`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub capabilities: BTreeMap<String, Vec<String>>,
    /// Cross-language bridges (0.6.1): how a Rust function is exposed and
    /// how TypeScript calls it, so each call becomes an edge in the map.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bridges: Vec<Bridge>,
}

/// One bridge kind (`tauri-command`): `rust` is a regex for the marker
/// line before the exposed `fn` (`#[tauri::command`); `ts` is a regex
/// for the call, `NAME` standing for the function's name
/// (`invoke(s*['"]NAME['"]`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Bridge {
    pub name: String,
    pub rust: String,
    pub ts: String,
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

    /// Every installed plugin that loads, and why each one that does not
    /// failed (0.5.9: one broken pack used to hide every rule, the
    /// project's own included). A pack's folder must be named after it.
    pub fn load_plugins_lenient(&self) -> (Vec<Installed>, Vec<String>) {
        let dir = self.plugins_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return (Vec::new(), Vec::new());
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        let mut ok = Vec::new();
        let mut broken = Vec::new();
        for d in dirs {
            match load_one(&d) {
                Ok(p) => ok.push(p),
                Err(e) => broken.push(format!("{e:#}")),
            }
        }
        (ok, broken)
    }

    /// Every installed plugin; the first broken one is an error (the
    /// doctor and `plugin` commands want to know).
    pub fn load_plugins(&self) -> anyhow::Result<Vec<Installed>> {
        let (ok, broken) = self.load_plugins_lenient();
        match broken.into_iter().next() {
            Some(e) => Err(anyhow::anyhow!(e)),
            None => Ok(ok),
        }
    }

    /// The project's rules plus every installed plugin's, each plugin
    /// rule namespaced `<plugin>/<id>` and marked with its origin. The one
    /// read every check and briefing uses; edits still go to `rules.toml`
    /// (`load_rules` / `save_rules`) and never write a plugin's rules.
    ///
    /// A project rule carrying a plugin rule's full id
    /// (`gpui-kit/stable-identity`) overrides it (0.5.9): `harna
    /// deprecate-rule` writes such an override, so a plugin convention can
    /// be switched off without editing the pack. A broken pack is skipped
    /// here; validate and the doctor name it.
    pub fn load_effective_rules(&self) -> anyhow::Result<Rules> {
        let mut rules = self.load_rules()?;
        for p in self.load_plugins_lenient().0 {
            for mut r in p.rules.rules {
                r.id = format!("{}/{}", p.meta.name, r.id);
                if rules.rules.iter().any(|x| x.id == r.id) {
                    continue; // the project's override wins
                }
                r.origin = format!("plugin {} {}", p.meta.name, p.meta.version);
                rules.rules.push(r);
            }
        }
        Ok(rules)
    }

    /// Before a lifecycle edit of `id`: a plugin rule not yet overridden
    /// is copied into `rules` as the project's override (0.5.9), so
    /// `deprecate-rule gpui-kit/x` works and survives `plugin add`.
    /// Returns whether a copy was made.
    pub fn adopt_plugin_rule(&self, rules: &mut Rules, id: &str) -> bool {
        if rules.rules.iter().any(|r| r.id == id) {
            return false;
        }
        match self.plugin_rule(id) {
            Some(r) => {
                rules.rules.push(r);
                true
            }
            None => false,
        }
    }

    /// Drop the project's copy of a plugin rule when it no longer differs
    /// from the pack's (ignoring its origin); returns whether it did.
    pub fn drop_redundant_override(&self, rules: &mut Rules, id: &str) -> bool {
        let Some(pack) = self.plugin_rule(id) else {
            return false;
        };
        let same = |r: &crate::schema::model::rules::Rule| {
            let mut a = r.clone();
            let mut b = pack.clone();
            a.origin.clear();
            b.origin.clear();
            serde_json::to_string(&a).ok() == serde_json::to_string(&b).ok()
        };
        let before = rules.rules.len();
        rules.rules.retain(|r| !(r.id == id && same(r)));
        rules.rules.len() != before
    }

    /// One plugin rule, namespaced, as its pack carries it.
    pub fn plugin_rule(&self, full_id: &str) -> Option<crate::schema::model::rules::Rule> {
        let (name, id) = full_id.split_once('/')?;
        let p = self
            .load_plugins_lenient()
            .0
            .into_iter()
            .find(|p| p.meta.name == name)?;
        let mut r = p.rules.rules.into_iter().find(|r| r.id == id)?;
        r.id = full_id.to_string();
        r.origin = format!("plugin {} {}", p.meta.name, p.meta.version);
        Some(r)
    }

    /// The glossary plus the plugins' vocabularies (a plugin's word never
    /// replaces the project's own).
    pub fn load_effective_terms(&self) -> anyhow::Result<Terms> {
        let mut terms = self.load_terms()?;
        for p in self.load_plugins_lenient().0 {
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

/// A plugin name is a plain word (0.5.9: `C:` joined onto the plugins
/// folder escaped it on Windows): lowercase letters, digits, `-`, `_`.
pub fn valid_plugin_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn load_one(d: &std::path::Path) -> anyhow::Result<Installed> {
    let read = |f: &str| std::fs::read_to_string(d.join(f)).ok();
    let meta: PluginMeta = toml::from_str(
        &read("plugin.toml").with_context(|| format!("{}: no plugin.toml", d.display()))?,
    )
    .with_context(|| format!("{}/plugin.toml is not valid", d.display()))?;
    let folder = d.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !valid_plugin_name(&meta.name) || folder != meta.name {
        anyhow::bail!(
            "{}: plugin `{}` must live in a folder of the same plain name (lowercase, digits, - _)",
            d.display(),
            meta.name
        );
    }
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
    // a pack written for a newer harna is refused, not misread (0.9.0)
    super::check_schema_version(rules.schema_version.max(terms.schema_version))
        .with_context(|| format!("plugin {}", d.display()))?;
    let api: Option<Api> = match read("api.toml") {
        Some(t) => Some(
            toml::from_str(&t).with_context(|| format!("{}/api.toml is not valid", d.display()))?,
        ),
        None => None,
    };
    Ok(Installed {
        meta,
        dir: d.to_path_buf(),
        rules,
        terms,
        api,
    })
}
