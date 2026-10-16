//! Component `deny-sync`: deny.toml agrees with the manifest.
//!
//! `harna deny` renders the topology into deny.toml; if the manifest's layers
//! change afterwards, the rendered file is silently wrong — compile-time
//! layering would enforce the OLD graph. This check makes staleness visible
//! (Info) instead of silent.

use std::path::Path;

use super::{Severity, Violation};
use crate::schema::manifest::Manifest;

/// Compare deny.toml on disk against a fresh render of the manifest.
pub fn check(root: &Path, manifest: &Manifest, out: &mut Vec<Violation>) {
    if manifest.layers.is_empty() {
        return; // no topology declared yet: nothing to sync
    }
    let rendered = match crate::generation::deny::plan(root, manifest)
        .map(|plan| crate::generation::deny::render_toml(&plan))
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
    s.trim_end().lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::manifest::{Container, Layer};

    fn manifest() -> Manifest {
        Manifest {
            schema_version: 1,
            layers: vec![Layer {
                name: "l".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                containers: vec![Container {
                    name: "c".into(),
                    path: String::new(),
                    description: String::new(),
                    sequence: vec![],
                    provides: vec![],
                    role: None,
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

        // Declared layers + no deny.toml: one Info.
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
