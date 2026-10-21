//! Component `plugin` (0.5.0): framework knowledge as versioned packs.
//!
//! A plugin folder holds `plugin.toml` (name, version, the upstream it
//! mirrors, its sources) and any of `rules.toml`, `terms.toml`. `harna
//! plugin add <name|path>` vendors it into `.harna/plugins/<name>/`, where
//! its rules join the project's own (namespaced `<plugin>/<id>`) — see
//! `schema::store::plugins`. Re-adding upgrades in place; `remove` takes
//! it out. Where plugins come from, in order: a folder path; a plugin
//! shipped with harna (`crates/harna-core/plugins/`, embedded); a folder
//! under `~/.harna/plugins/`. The doctor compares what a plugin mirrors
//! with `Cargo.lock`, so a framework upgrade never silently outruns its
//! conventions.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::schema::store::plugins::PluginMeta;
use crate::schema::store::Store;

/// The files a plugin may carry besides `plugin.toml`.
pub const FILES: &[&str] = &["rules.toml", "terms.toml", "api.toml"];

/// A plugin as found (not yet installed).
#[derive(Debug, Clone)]
pub struct Pack {
    pub meta: PluginMeta,
    /// (file name, contents), `plugin.toml` included.
    pub files: Vec<(String, String)>,
    pub source: String,
}

fn shipped_raw() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            "gpui-kit",
            vec![
                (
                    "plugin.toml",
                    include_str!("../../../plugins/gpui-kit/plugin.toml"),
                ),
                (
                    "rules.toml",
                    include_str!("../../../plugins/gpui-kit/rules.toml"),
                ),
                (
                    "terms.toml",
                    include_str!("../../../plugins/gpui-kit/terms.toml"),
                ),
                (
                    "api.toml",
                    include_str!("../../../plugins/gpui-kit/api.toml"),
                ),
            ],
        ),
        (
            "practices",
            vec![
                (
                    "plugin.toml",
                    include_str!("../../../plugins/practices/plugin.toml"),
                ),
                (
                    "rules.toml",
                    include_str!("../../../plugins/practices/rules.toml"),
                ),
            ],
        ),
        (
            "tauri",
            vec![
                (
                    "plugin.toml",
                    include_str!("../../../plugins/tauri/plugin.toml"),
                ),
                (
                    "rules.toml",
                    include_str!("../../../plugins/tauri/rules.toml"),
                ),
                (
                    "terms.toml",
                    include_str!("../../../plugins/tauri/terms.toml"),
                ),
            ],
        ),
    ]
}

fn from_parts(files: Vec<(String, String)>, source: String) -> anyhow::Result<Pack> {
    let meta_raw = files
        .iter()
        .find(|(f, _)| f == "plugin.toml")
        .map(|(_, t)| t.clone())
        .with_context(|| format!("{source}: a plugin needs a plugin.toml"))?;
    let meta: PluginMeta =
        toml::from_str(&meta_raw).with_context(|| format!("{source}: plugin.toml is not valid"))?;
    crate::schema::store::plugins::check_interface(&meta).with_context(|| source.clone())?;
    if !crate::schema::store::plugins::valid_plugin_name(&meta.name) {
        anyhow::bail!(
            "{source}: plugin name `{}` must be a plain word (lowercase letters, digits, - _)",
            meta.name
        );
    }
    for (f, t) in &files {
        validate(f, t).with_context(|| format!("{source}: {f}"))?;
    }
    // a bridge is two regexes; both must compile (0.6.1)
    for b in &meta.bridges {
        regex::Regex::new(&b.rust)
            .and_then(|_| regex::Regex::new(&b.ts.replace("NAME", "x")))
            .map_err(|e| anyhow::anyhow!("{source}: bridge `{}`: {e}", b.name))?;
        if !b.ts.contains("NAME") {
            anyhow::bail!("{source}: bridge `{}`: ts must contain NAME", b.name);
        }
    }
    Ok(Pack {
        meta,
        files,
        source,
    })
}

/// Each file must load as harna reads it before anything reaches
/// `.harna/`; the edges also admit the rules against the check registry
/// (`checks::admit_pack`) — a layer this one may not reach.
fn validate(file: &str, text: &str) -> anyhow::Result<()> {
    match file {
        // a pack from a newer harna is refused before it is installed (1.0)
        "rules.toml" => crate::schema::store::check_schema_version(
            toml::from_str::<crate::schema::model::rules::Rules>(text)?.schema_version,
        ),
        "terms.toml" => crate::schema::store::check_schema_version(
            toml::from_str::<crate::schema::registers::terms::Terms>(text)?.schema_version,
        ),
        "api.toml" => toml::from_str::<crate::schema::store::plugins::Api>(text)
            .map(|_| ())
            .map_err(Into::into),
        _ => Ok(()),
    }
}

impl Pack {
    /// The pack's rules (empty when it carries none).
    pub fn rules(&self) -> crate::schema::model::rules::Rules {
        self.files
            .iter()
            .find(|(f, _)| f == "rules.toml")
            .and_then(|(_, t)| toml::from_str(t).ok())
            .unwrap_or_default()
    }
}

pub fn shipped() -> Vec<Pack> {
    shipped_raw()
        .into_iter()
        .filter_map(|(name, files)| {
            from_parts(
                files
                    .into_iter()
                    .map(|(f, t)| (f.to_string(), t.to_string()))
                    .collect(),
                format!("shipped:{name}"),
            )
            .ok()
        })
        .collect()
}

pub fn from_dir(dir: &Path) -> anyhow::Result<Pack> {
    let mut files = Vec::new();
    for f in std::iter::once("plugin.toml").chain(FILES.iter().copied()) {
        let p = dir.join(f);
        if p.is_file() {
            files.push((f.to_string(), std::fs::read_to_string(&p)?));
        }
    }
    from_parts(files, dir.display().to_string())
}

/// The user's own plugins folder: `~/.harna/plugins`.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".harna").join("plugins"))
}

/// A path, a shipped name, or a name under `~/.harna/plugins`.
pub fn resolve(spec: &str, home: Option<&Path>) -> anyhow::Result<Pack> {
    // a plain name means the shipped pack even when a folder of that name
    // sits in the working directory (0.5.9); a path says so with a slash
    let plain = !spec.contains(['/', '\\']);
    if plain {
        if let Some(p) = shipped().into_iter().find(|p| p.meta.name == spec) {
            return Ok(p);
        }
    }
    let as_path = Path::new(spec);
    if as_path.is_dir() {
        return from_dir(as_path);
    }
    if let Some(dir) = home.map(|h| h.join(spec)).filter(|d| d.is_dir()) {
        return from_dir(&dir);
    }
    let known: Vec<String> = available(home).into_iter().map(|p| p.meta.name).collect();
    anyhow::bail!(
        "no plugin `{spec}` — available: {} (or pass a path to a plugin folder)",
        if known.is_empty() {
            "none".into()
        } else {
            known.join(", ")
        }
    )
}

/// Every plugin reachable without a path: shipped, then the user's.
pub fn available(home: Option<&Path>) -> Vec<Pack> {
    let mut out = shipped();
    if let Some(dir) = home {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten().filter(|e| e.path().is_dir()) {
                if let Ok(p) = from_dir(&e.path()) {
                    if !out.iter().any(|o| o.meta.name == p.meta.name) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// Vendor the pack into `.harna/plugins/<name>/` (an upgrade replaces the
/// folder's files; files the new version no longer carries are removed).
pub fn add(store: &Store, pack: &Pack) -> anyhow::Result<Vec<String>> {
    let dir = store.plugins_dir().join(&pack.meta.name);
    let before = store
        .load_plugins()
        .ok()
        .and_then(|ps| ps.into_iter().find(|p| p.meta.name == pack.meta.name))
        .map(|p| p.meta.version);
    std::fs::create_dir_all(&dir)?;
    for f in FILES {
        if !pack.files.iter().any(|(n, _)| n == f) && dir.join(f).exists() {
            std::fs::remove_file(dir.join(f))?;
        }
    }
    for (f, t) in &pack.files {
        crate::schema::store::atomic::write(dir.join(f), t)?;
    }
    let count = |f: &str, key: &str| {
        pack.files
            .iter()
            .find(|(n, _)| n == f)
            .map(|(_, t)| t.lines().filter(|l| l.trim() == key).count())
            .unwrap_or(0)
    };
    let mut out = vec![format!(
        "plugin `{}` {} {} ({}): {} rule(s), {} term(s) — namespaced `{}/<id>` beside your rules",
        pack.meta.name,
        pack.meta.version,
        match before {
            Some(v) if v == pack.meta.version => "re-added".to_string(),
            Some(v) => format!("upgraded from {v}"),
            None => "added".to_string(),
        },
        pack.source,
        count("rules.toml", "[[rules]]"),
        count("terms.toml", "[[terms]]"),
        pack.meta.name
    )];
    for (upstream, v) in &pack.meta.mirrors {
        out.push(format!("  mirrors {upstream} {v}"));
    }
    Ok(out)
}

pub fn remove(store: &Store, name: &str) -> anyhow::Result<String> {
    if !crate::schema::store::plugins::valid_plugin_name(name) {
        anyhow::bail!("`{name}` is not a plugin name (lowercase letters, digits, - _)");
    }
    let dir = store.plugins_dir().join(name);
    if !dir.join("plugin.toml").is_file() {
        anyhow::bail!("no installed plugin `{name}` (harna plugin list)");
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(format!(
        "plugin `{name}` removed — its rules and terms no longer apply"
    ))
}

/// What a plugin carries, for `harna plugin show`.
pub fn describe(p: &Pack) -> String {
    let mut out = format!(
        "{} {} — {}\n",
        p.meta.name, p.meta.version, p.meta.description
    );
    for (upstream, v) in &p.meta.mirrors {
        out.push_str(&format!("mirrors: {upstream} {v}\n"));
    }
    for b in &p.meta.bridges {
        out.push_str(&format!(
            "bridge {}: Rust `{}` <-> TS `{}`\n",
            b.name, b.rust, b.ts
        ));
    }
    for (cap, toks) in &p.meta.capabilities {
        out.push_str(&format!(
            "capability {}:{cap} — owns {}\n",
            p.meta.name,
            toks.join(" ")
        ));
    }
    for s in &p.meta.sources {
        out.push_str(&format!("source: {s}\n"));
    }
    for (f, t) in &p.files {
        match f.as_str() {
            "rules.toml" => {
                if let Ok(r) = toml::from_str::<crate::schema::model::rules::Rules>(t) {
                    out.push_str("\nrules:\n");
                    for x in &r.rules {
                        out.push_str(&format!(
                            "  {}/{}{} — {}\n",
                            p.meta.name,
                            x.id,
                            x.check
                                .as_deref()
                                .map(|c| format!(" [{c}]"))
                                .unwrap_or_default(),
                            x.text
                        ));
                    }
                }
            }
            "terms.toml" => {
                if let Ok(ts) = toml::from_str::<crate::schema::registers::terms::Terms>(t) {
                    out.push_str("\nterms: ");
                    out.push_str(
                        &ts.terms
                            .iter()
                            .map(|x| x.word.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    out
}

/// Generate a plugin's `api.toml` (0.5.2) from released source: `mounts`
/// are (prefix, crate `src/` dir) — "" for the crate's own root, a name
/// for a crate it re-exports as a module (`component` = gpui-component).
/// Run by a maintainer on upgrade; the gate only ever reads the result.
pub fn write_api(
    plugin_dir: &Path,
    crate_name: &str,
    version: &str,
    mounts: &[(String, PathBuf)],
) -> anyhow::Result<String> {
    let mut modules = Vec::new();
    for (prefix, src) in mounts {
        if !src.join("lib.rs").is_file() {
            anyhow::bail!("{}: no lib.rs — pass a crate's src/ folder", src.display());
        }
        modules.extend(crate::search::digest::api::surface(src, prefix));
    }
    // a mount's root module is also an item of the facade's root
    let mounted: Vec<String> = mounts
        .iter()
        .filter(|(p, _)| !p.is_empty())
        .map(|(p, _)| p.clone())
        .collect();
    if let Some(root) = modules.iter_mut().find(|m| m.path.is_empty()) {
        root.items.extend(mounted);
        root.items.sort();
        root.items.dedup();
    }
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    let items: usize = modules.iter().map(|m| m.items.len()).sum();
    let api = crate::schema::store::plugins::Api {
        crate_name: crate_name.into(),
        version: version.into(),
        modules,
    };
    let header = format!(
        "# Generated by `harna plugin api` from {crate_name} {version}'s released source.\n# Re-generate on upgrade; the gate reads it offline (check api-parity).\n"
    );
    crate::schema::store::atomic::write(
        plugin_dir.join("api.toml"),
        format!("{header}{}", toml::to_string(&api)?),
    )?;
    Ok(format!(
        "api.toml: {} module(s), {items} item(s) for {crate_name} {version}",
        api.modules.len()
    ))
}

/// Mirrored Cargo packages whose locked version left the plugin's
/// (major.minor in 0.x, major otherwise): `(plugin, package, mirrored, locked)`.
pub fn drift(root: &Path, store: &Store) -> Vec<(String, String, String, String)> {
    let Ok(lock) = std::fs::read_to_string(root.join("Cargo.lock")) else {
        return Vec::new();
    };
    let Ok(lock) = toml::from_str::<toml::Value>(&lock) else {
        return Vec::new();
    };
    let locked = |name: &str| -> Vec<String> {
        lock.get("package")
            .and_then(|p| p.as_array())
            .map(|ps| {
                ps.iter()
                    .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
                    .filter_map(|p| p.get("version")?.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let line = |v: &str| -> String {
        // a requirement (`^0.8`, `~0.8.1`, `>=0.8`) names its line too (0.5.9)
        let v = v.trim_start_matches(['^', '~', '=', '>', '<', ' ']);
        let parts: Vec<&str> = v.split('.').collect();
        match parts.as_slice() {
            ["0", minor, ..] => format!("0.{minor}"),
            [major, ..] => major.to_string(),
            [] => v.to_string(),
        }
    };
    let mut out = Vec::new();
    for p in store.load_plugins().unwrap_or_default() {
        for (pkg, want) in &p.meta.mirrors {
            for have in locked(pkg) {
                if line(&have) != line(want) {
                    out.push((p.meta.name.clone(), pkg.clone(), want.clone(), have));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
