//! Component `owners` (0.7.0–0.7.1): who answers for a change.
//!
//! Owners are data on layers and containers (`harna container set <c>
//! --owners @ledger-team`). `harna owners <path>…` names who answers for
//! each path; `--changed` routes the working tree's change to the people
//! who must look — grouped per owner, so a review request is one line.
//! Paths nobody owns are named too: an unowned corner is a finding, not a
//! silence.

use std::collections::BTreeMap;
use std::path::Path;

use crate::schema::model::manifest::{owners_of, Manifest};

pub struct Routing {
    /// owner -> the paths they answer for
    pub by_owner: BTreeMap<String, Vec<String>>,
    pub unowned: Vec<String>,
}

pub fn route(manifest: &Manifest, paths: &[String]) -> Routing {
    let mut by_owner: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut unowned = Vec::new();
    for p in paths {
        let p = p.replace('\\', "/");
        let owners = owners_of(manifest, &p);
        if owners.is_empty() {
            unowned.push(p);
            continue;
        }
        for o in owners {
            by_owner.entry(o).or_default().push(p.clone());
        }
    }
    Routing { by_owner, unowned }
}

/// The working tree's changed files (git), routed.
pub fn route_changed(root: &Path, manifest: &Manifest) -> anyhow::Result<Routing> {
    let files = crate::enforcement::validate::git::changed_files(root)?;
    Ok(route(manifest, &files))
}

impl Routing {
    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.by_owner.is_empty() && self.unowned.is_empty() {
            return "no paths to route\n".into();
        }
        for (o, paths) in &self.by_owner {
            out.push_str(&format!("{o} — {} path(s)\n", paths.len()));
            for p in paths.iter().take(12) {
                out.push_str(&format!("  {p}\n"));
            }
            if paths.len() > 12 {
                out.push_str(&format!("  … {} more\n", paths.len() - 12));
            }
        }
        if !self.unowned.is_empty() {
            out.push_str(&format!(
                "unowned — {} path(s) (harna container set <c> --owners @team):\n",
                self.unowned.len()
            ));
            for p in self.unowned.iter().take(12) {
                out.push_str(&format!("  {p}\n"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};

    /// 0.7.0: the innermost container naming owners answers, then the
    /// layer; a path nobody holds is unowned.
    #[test]
    fn the_innermost_owner_answers() {
        let c = |name: &str, path: &str, owners: &[&str]| Container {
            name: name.into(),
            path: path.into(),
            owners: owners.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let m = Manifest {
            layers: vec![Layer {
                name: "core".into(),
                owners: vec!["@core".into()],
                containers: vec![
                    c("ledger", "crates/ledger/src", &["@ledger"]),
                    c("fx", "crates/ledger/src/fx", &[]),
                    c("money", "crates/money/src", &[]),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let r = route(
            &m,
            &[
                "crates/ledger/src/post.rs".into(),
                "crates/ledger/src/fx/rate.rs".into(),
                "crates/money/src/lib.rs".into(),
                "docs/x.md".into(),
            ],
        );
        assert_eq!(
            r.by_owner["@ledger"],
            vec!["crates/ledger/src/post.rs", "crates/ledger/src/fx/rate.rs"],
            "an ownerless nested container falls to its enclosing one"
        );
        assert_eq!(r.by_owner["@core"], vec!["crates/money/src/lib.rs"]);
        assert_eq!(r.unowned, vec!["docs/x.md"]);
    }
}
