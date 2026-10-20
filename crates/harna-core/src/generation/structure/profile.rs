//! Component `profile` (0.4.2): starting points as data.
//!
//! A profile is a folder: `profile.toml` (name, description, the upstream
//! versions it mirrors, next steps) plus any of the registers a project
//! starts from — manifest, rules, qualities, terms, commands, non-goals.
//! `harna init --profile <name|path>` writes them into `.harna/`; harna
//! never generates code from a profile, and a profile never decides the
//! product: it is the shape a kind of project usually starts with, to
//! prune (rule `tools-not-prescriptions`).
//!
//! Where profiles come from, in order: a path to a folder; a profile
//! shipped with harna (`crates/harna-core/profiles/`, embedded); a folder
//! under `~/.harna/profiles/`. `{{project}}` (kebab) and
//! `{{project_snake}}` in any file become the repository's name.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

use crate::schema::store::Store;

/// The registers a profile may carry, in write order.
pub const FILES: &[&str] = &[
    "manifest.toml",
    "rules.toml",
    "qualities.toml",
    "terms.toml",
    "commands.toml",
    "non-goals.toml",
];

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Meta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The profile's own version (it changes as its upstream does).
    #[serde(default)]
    pub version: String,
    /// Upstream versions the profile mirrors, e.g. `gpui-kit = "0.8.0"`.
    #[serde(default)]
    pub mirrors: BTreeMap<String, String>,
    /// What to do after init, printed in order.
    #[serde(default)]
    pub next: Vec<String>,
    /// Plugins the profile installs (0.5.0): the framework knowledge it
    /// builds on, kept out of the profile itself.
    #[serde(default)]
    pub plugins: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub meta: Meta,
    /// (register file name, contents) — only the registers present.
    pub files: Vec<(String, String)>,
    /// Where it came from: "shipped", a path, or the user's profiles dir.
    pub source: String,
}

/// Profiles shipped with harna: (folder name, [(file, contents)]).
fn shipped_raw() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            "rust-gpui-desktop",
            vec![
                (
                    "profile.toml",
                    include_str!("../../../profiles/rust-gpui-desktop/profile.toml"),
                ),
                (
                    "manifest.toml",
                    include_str!("../../../profiles/rust-gpui-desktop/manifest.toml"),
                ),
                (
                    "rules.toml",
                    include_str!("../../../profiles/rust-gpui-desktop/rules.toml"),
                ),
                (
                    "qualities.toml",
                    include_str!("../../../profiles/rust-gpui-desktop/qualities.toml"),
                ),
                (
                    "terms.toml",
                    include_str!("../../../profiles/rust-gpui-desktop/terms.toml"),
                ),
            ],
        ),
        (
            "tauri-react",
            vec![
                (
                    "profile.toml",
                    include_str!("../../../profiles/tauri-react/profile.toml"),
                ),
                (
                    "manifest.toml",
                    include_str!("../../../profiles/tauri-react/manifest.toml"),
                ),
                (
                    "rules.toml",
                    include_str!("../../../profiles/tauri-react/rules.toml"),
                ),
                (
                    "terms.toml",
                    include_str!("../../../profiles/tauri-react/terms.toml"),
                ),
            ],
        ),
    ]
}

fn from_parts(files: Vec<(String, String)>, source: String) -> anyhow::Result<Profile> {
    let meta_raw = files
        .iter()
        .find(|(f, _)| f == "profile.toml")
        .map(|(_, t)| t.clone())
        .with_context(|| format!("{source}: a profile needs a profile.toml"))?;
    let meta: Meta = toml::from_str(&meta_raw)
        .with_context(|| format!("{source}: profile.toml is not valid"))?;
    let files = files
        .into_iter()
        .filter(|(f, _)| FILES.contains(&f.as_str()))
        .collect();
    Ok(Profile {
        meta,
        files,
        source,
    })
}

pub fn shipped() -> Vec<Profile> {
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

/// Read a profile folder from disk.
pub fn from_dir(dir: &Path) -> anyhow::Result<Profile> {
    let mut files = Vec::new();
    for f in std::iter::once("profile.toml").chain(FILES.iter().copied()) {
        let p = dir.join(f);
        if p.is_file() {
            files.push((f.to_string(), std::fs::read_to_string(&p)?));
        }
    }
    from_parts(files, dir.display().to_string())
}

/// The user's own profiles folder: `~/.harna/profiles`.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".harna").join("profiles"))
}

/// A path, a shipped name, or a name under `~/.harna/profiles`.
pub fn resolve(spec: &str, home: Option<&Path>) -> anyhow::Result<Profile> {
    let as_path = Path::new(spec);
    if as_path.is_dir() {
        return from_dir(as_path);
    }
    if let Some(p) = shipped().into_iter().find(|p| p.meta.name == spec) {
        return Ok(p);
    }
    if let Some(dir) = home.map(|h| h.join(spec)).filter(|d| d.is_dir()) {
        return from_dir(&dir);
    }
    let known: Vec<String> = list(home).into_iter().map(|p| p.meta.name).collect();
    anyhow::bail!(
        "no profile `{spec}` — known: {} (or pass a path to a profile folder)",
        if known.is_empty() {
            "none".into()
        } else {
            known.join(", ")
        }
    )
}

/// Every profile reachable without a path: shipped, then the user's.
pub fn list(home: Option<&Path>) -> Vec<Profile> {
    let mut out = shipped();
    if let Some(dir) = home {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten().filter(|e| e.path().is_dir()) {
                if let Ok(p) = from_dir(&e.path()) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// `{{project}}` and `{{project_snake}}` from the repository's folder name.
fn substitute(text: &str, root: &Path) -> String {
    let name = root
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "app".into());
    // a valid crate name (0.4.9): ASCII letters/digits, no leading digit,
    // never empty — `Ünïcode 2go` -> `n-code-2go`, `123` -> `app-123`
    let mut kebab: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if kebab.is_empty() {
        kebab = "app".into();
    } else if kebab.starts_with(|c: char| c.is_ascii_digit()) {
        kebab = format!("app-{kebab}");
    }
    text.replace("{{project_snake}}", &kebab.replace('-', "_"))
        .replace("{{project}}", &kebab)
}

/// Each register must load as harna reads it, before anything is written.
fn validate(file: &str, text: &str) -> anyhow::Result<()> {
    use crate::schema::{model, registers};
    let r = match file {
        "manifest.toml" => toml::from_str::<model::manifest::Manifest>(text).map(|_| ()),
        "rules.toml" => toml::from_str::<model::rules::Rules>(text).map(|_| ()),
        "qualities.toml" => toml::from_str::<registers::qualities::Qualities>(text).map(|_| ()),
        "terms.toml" => toml::from_str::<registers::terms::Terms>(text).map(|_| ()),
        "commands.toml" => toml::from_str::<registers::commands::Commands>(text).map(|_| ()),
        "non-goals.toml" => toml::from_str::<registers::non_goals::NonGoals>(text).map(|_| ()),
        _ => return Ok(()),
    };
    r.with_context(|| format!("{file} in the profile is not valid — profile refused"))
}

/// Write the profile into `.harna/`. The manifest and rules replace the
/// fresh starters; any other register is written only where none exists,
/// so a profile never overwrites a project's own data.
pub fn apply(store: &Store, root: &Path, p: &Profile) -> anyhow::Result<Vec<String>> {
    let prepared: Vec<(String, String)> = p
        .files
        .iter()
        .map(|(f, t)| (f.clone(), substitute(t, root)))
        .collect();
    for (f, t) in &prepared {
        validate(f, t)?;
    }
    store.init()?;
    let dir = store
        .manifest_path()
        .parent()
        .map(Path::to_path_buf)
        .context("store root")?;
    let mut out = vec![format!(
        "profile `{}`{} ({})",
        p.meta.name,
        if p.meta.version.is_empty() {
            String::new()
        } else {
            format!(" v{}", p.meta.version)
        },
        p.source
    )];
    for (f, t) in prepared {
        let target = dir.join(&f);
        let replace = matches!(f.as_str(), "manifest.toml" | "rules.toml");
        if !replace && target.exists() {
            out.push(format!(
                "  kept your {f} (a profile never overwrites a register you have)"
            ));
            continue;
        }
        crate::schema::store::atomic::write(&target, t)?;
        out.push(format!("  wrote .harna/{f}"));
    }
    for (upstream, v) in &p.meta.mirrors {
        out.push(format!("  mirrors {upstream} {v}"));
    }
    for name in &p.meta.plugins {
        let pack = super::plugin::resolve(name, super::plugin::home_dir().as_deref())?;
        for line in super::plugin::add(store, &pack)? {
            out.push(format!("  {line}"));
        }
    }
    for (i, step) in p.meta.next.iter().enumerate() {
        out.push(format!("  next {}: {}", i + 1, substitute(step, root)));
    }
    Ok(out)
}

/// What a profile declares, for `harna profile show`: its layers (with
/// what each may import), its rules (and the checks they bind), the other
/// registers it carries, and its next steps.
pub fn describe(p: &Profile) -> String {
    let mut out = format!("{} — {}\n", p.meta.name, p.meta.description);
    for (upstream, v) in &p.meta.mirrors {
        out.push_str(&format!("mirrors: {upstream} {v}\n"));
    }
    for (file, text) in &p.files {
        out.push_str(&format!("\n{file}:\n"));
        match file.as_str() {
            "manifest.toml" => {
                if let Ok(m) = toml::from_str::<crate::schema::model::manifest::Manifest>(text) {
                    for l in &m.layers {
                        let deps = if l.depends_on.is_empty() {
                            "nothing".into()
                        } else {
                            l.depends_on.join(", ")
                        };
                        out.push_str(&format!(
                            "  layer {}{} — may import {deps}: {}\n",
                            l.name,
                            if l.isolated { " (isolated)" } else { "" },
                            l.description
                        ));
                    }
                }
            }
            "rules.toml" => {
                if let Ok(r) = toml::from_str::<crate::schema::model::rules::Rules>(text) {
                    for rule in &r.rules {
                        let bound = rule
                            .check
                            .as_deref()
                            .map(|c| format!(" [check: {c}]"))
                            .unwrap_or_default();
                        out.push_str(&format!("  rule {}{bound}: {}\n", rule.id, rule.text));
                    }
                }
            }
            _ => out.push_str(&format!("  {} line(s)\n", text.lines().count())),
        }
    }
    for (i, s) in p.meta.next.iter().enumerate() {
        out.push_str(&format!("next {}: {s}\n", i + 1));
    }
    out
}

#[cfg(test)]
mod tests;
