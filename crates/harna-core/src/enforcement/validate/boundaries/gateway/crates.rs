//! `gateway-crates` (0.3.48, codex/zed/ruff study R2): gateways for
//! external crates. codex's `deny.toml` lets only `codex-http-client` wrap
//! `reqwest` ("its intended owner"). Here that is a facility like `fs`:
//! `gateway = "crate:reqwest"` on a container makes it the only place
//! whose crate may depend on `reqwest` in Cargo.toml and whose modules may
//! name `reqwest::`. Declaring it is the adoption switch, as for every
//! gateway; `harna deny` renders the owner as cargo-deny `wrappers`.

use std::path::Path;

use super::super::{Severity, Violation};
use crate::schema::model::manifest::Manifest;
use crate::search::digest::crates::CrateIndex;
use crate::search::digest::live::live_code;

/// Whether a crate dir and an owner container belong together: the
/// container inside the crate, or the crate inside the container.
fn crate_owns(dir: &str, owner: &str) -> bool {
    let within = |a: &str, b: &str| b == "." || a == b || a.starts_with(&format!("{b}/"));
    within(owner, dir) || within(dir, owner)
}

pub fn check(
    root: &Path,
    manifest: &Manifest,
    srcs: &[(String, String)],
    out: &mut Vec<Violation>,
) {
    let owned = manifest.crate_gateways();
    if owned.is_empty() {
        return;
    }
    let index = CrateIndex::load(root);
    // Cargo: only the owner's crate may depend on it.
    for c in &index.crates {
        let deps = index.external_deps(root, &c.dir);
        for (name, paths) in &owned {
            if deps.iter().any(|d| d == name) && !paths.iter().any(|p| crate_owns(&c.dir, p)) {
                out.push(violation(
                    &format!("{}/Cargo.toml", c.dir).replace("./", ""),
                    0,
                    name,
                    paths,
                    "depends on",
                ));
            }
        }
    }
    // Source: only modules inside an owner container may name it.
    for (name, paths) in &owned {
        let ident = name.replace('-', "_");
        let tokens = [format!("{ident}::"), format!("use {ident}")];
        for (file, src) in srcs {
            if paths.iter().any(|p| file.starts_with(&format!("{p}/"))) {
                continue;
            }
            let hit = live_code(src).into_iter().find(|(_, line)| {
                tokens
                    .iter()
                    .any(|t| super::token_at(line, t).is_some_and(|at| bounded(line, at, t)))
            });
            if let Some((n, _)) = hit {
                out.push(violation(file, n, name, paths, "imports"));
            }
        }
    }
}

/// `use reqwest` must end the name (`use reqwest_middleware` is another).
fn bounded(line: &str, at: usize, token: &str) -> bool {
    token.ends_with("::")
        || line[at + token.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
}

fn violation(file: &str, line: usize, name: &str, owners: &[String], verb: &str) -> Violation {
    Violation {
        check: "gateway-ownership".into(),
        file: file.into(),
        line,
        severity: Severity::Error,
        message: format!(
            "{verb} `{name}`, which only its gateway may use ({})",
            owners.join(", ")
        ),
        hint: format!(
            "go through the owner's API, or move this code into it (manifest: gateway = \"crate:{name}\")"
        ),
    }
}

#[cfg(test)]
mod tests;
