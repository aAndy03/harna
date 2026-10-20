//! Component `toolbox` (0.3.50): harna's tools, visible in context.
//!
//! The catalog is data (`toolbox.toml`): what each tool does, when to reach
//! for it, how. `harna tools` lists it; `place`/`plan` offer the tools whose
//! `signal` holds for the containers they focus on. A suggestion is never a
//! requirement — the user's principle (rule `tools-not-prescriptions`): harna
//! shows the tools, the agent or engineer decides. Signals read structure
//! only (crates, imports, rules, tests), never a framework or product name.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::schema::model::manifest::{Container, Manifest};
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::path_within;
use crate::search::digest::Digest;

#[derive(Debug, Clone, Deserialize)]
pub struct Tool {
    pub id: String,
    pub title: String,
    pub does: String,
    pub reach_for: String,
    pub how: String,
    #[serde(default)]
    pub enforces: Option<String>,
    #[serde(default)]
    pub signal: Option<String>,
}

#[derive(Deserialize)]
struct Catalog {
    tools: Vec<Tool>,
}

/// The catalog, parsed once.
pub fn catalog() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        toml::from_str::<Catalog>(include_str!("toolbox.toml"))
            .expect("toolbox.toml must be valid data")
            .tools
    })
}

/// `harna tools all` and `harna://tools`: every tool, one block each.
/// The toolbox index (0.9.5): one line per tool; `harna tools <id>` is
/// one tool in full, `harna tools all` the whole catalog.
pub fn render_index() -> String {
    let mut out = format!(
        "harna's toolbox — {} optional tools; `harna tools <id>` for how to use one:\n",
        catalog().len()
    );
    for t in catalog() {
        out.push_str(&format!("  {:<20} {}\n", t.id, t.title));
    }
    out
}

/// One tool in full, or the whole catalog for `all`.
pub fn render_tool(id: &str) -> anyhow::Result<String> {
    if id == "all" {
        return Ok(render_catalog());
    }
    let t = catalog().iter().find(|t| t.id == id).ok_or_else(|| {
        anyhow::anyhow!(
            "no tool `{id}` — one of: {}",
            catalog()
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let mut out = format!(
        "{} [{}]\n  does: {}\n  reach for it: {}\n  how: {}\n",
        t.title, t.id, t.does, t.reach_for, t.how
    );
    if let Some(c) = &t.enforces {
        out.push_str(&format!("  checked by: {c}\n"));
    }
    Ok(out)
}

pub fn render_catalog() -> String {
    let mut out = String::from(
        "harna's toolbox — every tool is optional; reach for one when its moment comes:\n",
    );
    for t in catalog() {
        out.push_str(&format!(
            "\n{} [{}]\n  does: {}\n  reach for it: {}\n  how: {}\n",
            t.title, t.id, t.does, t.reach_for, t.how
        ));
        if let Some(c) = &t.enforces {
            out.push_str(&format!("  checked by: {c}\n"));
        }
    }
    out
}

/// The suggestions block for the containers a briefing focuses on: one
/// line per (tool, concrete reason), capped. Empty when nothing applies.
pub fn suggestions(
    root: &Path,
    manifest: &Manifest,
    rules: &Rules,
    digest: Option<&Digest>,
    focus: &[&str],
) -> String {
    const MAX: usize = 6;
    let containers: Vec<(&str, &Container)> = manifest
        .layers
        .iter()
        .flat_map(|l| l.containers.iter().map(move |c| (l.name.as_str(), c)))
        .filter(|(_, c)| focus.contains(&c.name.as_str()))
        .collect();
    if containers.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = Vec::new();
    for tool in catalog() {
        let Some(signal) = tool.signal.as_deref() else {
            continue;
        };
        let reasons = match signal {
            "external-crate-single-user" => single_user_crates(root, manifest, &containers),
            "imported-across-layers" => digest
                .map(|d| importer_layers(root, manifest, d, &containers))
                .map(|found| {
                    found
                        .into_iter()
                        .filter(|(_, from)| from.len() >= 2)
                        .map(|(c, from)| format!(
                            "`{c}` is imported directly from {} other layers ({}) — one corridor would own that hand-off",
                            from.len(),
                            from.join(", ")
                        ))
                        .collect()
                })
                .unwrap_or_default(),
            "imported-without-surface" => digest
                .map(|d| importer_layers(root, manifest, d, &containers))
                .map(|found| {
                    found
                        .into_iter()
                        .filter(|(c, from)| !from.is_empty() && !has_surface(&containers, c))
                        .map(|(c, from)| format!(
                            "`{c}` is imported from {} with no declared surface — every name in it is reachable",
                            from.join(", ")
                        ))
                        .collect()
                })
                .unwrap_or_default(),
            "no-rules-in-scope" => no_rules(rules, &containers),
            "untested-components" => untested(root, &containers),
            "no-qualities-declared" => no_qualities(root),
            _ => Vec::new(), // a signal this harna does not know yet
        };
        for r in reasons {
            lines.push(format!("  - {}: {r}\n    how: {}", tool.title, tool.how));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    lines.truncate(MAX);
    format!(
        "tools that fit here (optional — `harna tools` lists them all):\n{}\n",
        lines.join("\n")
    )
}

/// External crates only this container's crate depends on, not yet behind
/// a gateway: making it the owner keeps it that way.
fn single_user_crates(
    root: &Path,
    manifest: &Manifest,
    focus: &[(&str, &Container)],
) -> Vec<String> {
    let index = crate::search::digest::crates::CrateIndex::load(root);
    if index.crates.len() < 2 {
        return Vec::new(); // one crate: every dependency is "single-user"
    }
    let owned: BTreeSet<String> = manifest
        .crate_gateways()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let mut users: BTreeMap<String, usize> = BTreeMap::new();
    for c in &index.crates {
        for d in index.external_deps(root, &c.dir) {
            *users.entry(d).or_default() += 1;
        }
    }
    let mut out = Vec::new();
    for (_, c) in focus {
        let Some((info, _)) = index.locate_dir(&c.path) else {
            continue;
        };
        for d in index.external_deps(root, &info.dir) {
            if users.get(&d) == Some(&1) && !owned.contains(&d) {
                out.push(format!(
                    "only `{}` depends on `{d}` — owning it in `{}` keeps it from spreading",
                    info.package, c.name
                ));
            }
        }
    }
    out.truncate(2);
    out
}

/// Whether a container (or any of its components) declares `provides`.
fn has_surface(focus: &[(&str, &Container)], name: &str) -> bool {
    fn any(comps: &[crate::schema::model::manifest::Component]) -> bool {
        comps
            .iter()
            .any(|c| !c.provides.is_empty() || any(&c.children))
    }
    focus
        .iter()
        .find(|(_, c)| c.name == name)
        .is_some_and(|(_, c)| !c.provides.is_empty() || any(&c.components))
}

/// For each focus container: the other layers whose modules import it.
fn importer_layers(
    root: &Path,
    manifest: &Manifest,
    d: &Digest,
    focus: &[(&str, &Container)],
) -> Vec<(String, Vec<String>)> {
    use crate::search::digest;
    let layers = crate::schema::model::manifest::module_layers(manifest, d);
    let crate_names = digest::workspace_crate_names(root);
    let dirs: Vec<String> = d
        .modules
        .iter()
        .filter_map(|m| m.crate_of().0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let name_dirs = digest::crate_name_dirs(root, &dirs);
    let mut out = Vec::new();
    for (layer, c) in focus {
        let inside: Vec<&crate::search::digest::Module> = d
            .modules
            .iter()
            .filter(|m| !c.path.is_empty() && m.path.starts_with(&format!("{}/", c.path)))
            .collect();
        let mut from: BTreeSet<&str> = BTreeSet::new();
        for m in &d.modules {
            let Some(l) = layers.get(m.path.as_str()) else {
                continue;
            };
            if l == layer {
                continue;
            }
            let reaches = m.deps.iter().any(|dep| {
                inside
                    .iter()
                    .any(|t| digest::dep_reaches(m, dep, t, &crate_names, &name_dirs))
            });
            if reaches {
                from.insert(l);
            }
        }
        out.push((
            c.name.clone(),
            from.into_iter().map(str::to_string).collect(),
        ));
    }
    out
}

/// No live rule covers the container (and no global rule exists).
fn no_rules(rules: &Rules, focus: &[(&str, &Container)]) -> Vec<String> {
    let live: Vec<_> = rules.rules.iter().filter(|r| !r.is_deprecated()).collect();
    if live.iter().any(|r| r.scope.is_empty()) {
        return Vec::new();
    }
    focus
        .iter()
        .filter(|(_, c)| {
            !c.path.is_empty()
                && !live.iter().any(|r| {
                    r.scope
                        .iter()
                        .any(|s| path_within(&c.path, s) || path_within(s, &c.path))
                })
        })
        .map(|(_, c)| {
            format!(
                "no rule covers `{}` yet — conventions agreed for it can be recorded",
                c.name
            )
        })
        .take(1)
        .collect()
}

/// No long-term intent recorded yet: offered once per briefing.
fn no_qualities(root: &Path) -> Vec<String> {
    let empty = crate::schema::store::Store::open(root)
        .and_then(|s| s.load_qualities())
        .map(|q| q.is_empty())
        .unwrap_or(false);
    if empty {
        vec!["no qualities declared — how many users, how fast, offline or online, how much data loss is tolerable".into()]
    } else {
        Vec::new()
    }
}

/// Components without registered tests while their crate has integration
/// tests on disk.
fn untested(root: &Path, focus: &[(&str, &Container)]) -> Vec<String> {
    let index = crate::search::digest::crates::CrateIndex::load(root);
    let mut out = Vec::new();
    for (_, c) in focus {
        let Some((info, _)) = index.locate_dir(&c.path) else {
            continue;
        };
        let tests_dir = root.join(&info.dir).join("tests");
        let has_tests = std::fs::read_dir(&tests_dir)
            .map(|rd| {
                rd.flatten()
                    .any(|e| e.path().extension().is_some_and(|x| x == "rs"))
            })
            .unwrap_or(false);
        let bare = c.components.iter().filter(|k| k.tests.is_empty()).count();
        if has_tests && bare > 0 {
            out.push(format!(
                "{bare} component(s) of `{}` have no registered tests while `{}/tests/` exists",
                c.name, info.dir
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests;
