//! Component `deny-sync`: deny.toml agrees with the manifest.
//!
//! `harna deny` renders the topology into deny.toml; if the manifest's layers
//! change afterwards, the rendered file is silently wrong — compile-time
//! layering would enforce the OLD graph. This check makes staleness visible
//! (Info) instead of silent.

use std::path::Path;

use super::{Severity, Violation};
use crate::schema::model::manifest::Manifest;

/// Compare deny.toml on disk against a fresh render of the manifest.
pub fn check(root: &Path, manifest: &Manifest, out: &mut Vec<Violation>) {
    if manifest.layers.is_empty() {
        return; // no topology declared yet: nothing to sync
    }
    if !root.join("Cargo.toml").exists() {
        return; // no Cargo workspace (a TS-only repo, 0.6.0): cargo-deny has nothing to hold
    }
    let rendered = match crate::generation::views::deny::plan(root, manifest)
        .map(|plan| crate::generation::views::deny::render_toml(&plan))
    {
        Ok(r) => r,
        // Unresolvable crates (workspace metadata absent): skip quietly.
        Err(_) => return,
    };
    let on_disk = match std::fs::read_to_string(root.join("deny.toml")) {
        Ok(d) => d,
        Err(_) => {
            out.push(Violation {
                check: "deny-sync".into(),
                file: "deny.toml".into(),
                line: 0,
                message: "deny.toml not generated yet — compile-time layering is not enforced"
                    .into(),
                hint: "run `harna deny` once and commit the file; CI runs `cargo deny check bans`"
                    .into(),
                severity: Severity::Info,
            });
            return;
        }
    };
    // a hand-written cargo-deny config is the project's: it holds when it
    // bans what the layering needs (0.9.2: it was called stale forever)
    if !on_disk
        .trim_start()
        .starts_with(crate::generation::views::deny::HEADER)
    {
        let have = crate::generation::views::deny::banned_crates(&on_disk);
        let missing: Vec<String> = crate::generation::views::deny::plan(root, manifest)
            .map(|p| {
                p.rules
                    .into_iter()
                    .map(|r| r.crate_name)
                    .filter(|c| !have.contains(c))
                    .collect()
            })
            .unwrap_or_default();
        if !missing.is_empty() {
            out.push(Violation {
                check: "deny-sync".into(),
                file: "deny.toml".into(),
                line: 0,
                message: format!(
                    "the hand-written deny.toml bans none of {} — the layering is not enforced at compile time",
                    missing.join(", ")
                ),
                hint: "add an entry per crate to its [bans] deny list (harna deny names them)".into(),
                severity: Severity::Info,
            });
        }
        return;
    }
    if normalize(&on_disk) != normalize(&rendered) {
        out.push(Violation {
            check: "deny-sync".into(),
            file: "deny.toml".into(),
            line: 0,
            message: "deny.toml is stale — the manifest's layer topology changed since it was rendered"
                .into(),
            hint: "run `harna deny` and commit the regenerated file; CI would otherwise enforce the old graph"
                .into(),
            severity: Severity::Info,
        });
    }
}

/// End-of-file trailing whitespace/line endings do not count as staleness.
fn normalize(s: &str) -> String {
    s.trim_end()
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};

    fn manifest() -> Manifest {
        Manifest {
            test_kinds: Vec::new(),
            schema_version: 1,
            layers: vec![Layer {
                owners: Vec::new(),
                isolated: false,
                name: "l".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                role: None,
                containers: vec![Container {
                    owners: Vec::new(),
                    escapes: Vec::new(),
                    owns: Vec::new(),
                    name: "c".into(),
                    path: String::new(),
                    description: String::new(),
                    sequence: vec![],
                    provides: vec![],
                    role: None,
                    gateway: None,
                    connects: vec![],
                    components: vec![],
                }],
            }],
        }
    }

    #[test]
    fn missing_file_reported_but_layerless_manifests_are_quiet() {
        let tmp = std::env::temp_dir().join(format!("harna-denysync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Layerless manifest: silent.
        let m0 = Manifest::default();
        let mut v0 = Vec::new();
        check(&tmp, &m0, &mut v0);
        assert!(v0.is_empty());

        // No Cargo workspace (a TS-only repo): silent too.
        let mut v_ts = Vec::new();
        check(&tmp, &manifest(), &mut v_ts);
        assert!(v_ts.is_empty());

        // A Cargo repo with declared layers and no deny.toml: one Info.
        std::fs::write(tmp.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        let mut v1 = Vec::new();
        check(&tmp, &manifest(), &mut v1);
        assert_eq!(v1.len(), 1);
        assert!(v1[0].message.contains("not generated"));
    }

    #[test]
    fn staleness_comparison_ignores_trailing_whitespace() {
        let rendered = "# gen\n[bans]\n";
        let on_disk_trailing = "# gen\n[bans]\r\n\r\n";
        assert_eq!(normalize(on_disk_trailing), normalize(rendered));
        let stale = "# gen\n[bans]\nextra = true\n";
        assert_ne!(normalize(stale), normalize(rendered));
    }
}
