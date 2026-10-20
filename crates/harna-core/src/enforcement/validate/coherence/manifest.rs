//! Component `manifest`: coherence of the manifest itself — paths, sequences,
//! declared layer dependencies.
//!
//! These write-side checks stop drift at the source of truth before it can
//! leak into code checks.

use std::path::Path;

use super::{Severity, Violation};

/// Manifest drift: components whose `path` no longer exists on disk.
pub fn check_paths_exist(
    root: &Path,
    manifest: &crate::schema::model::manifest::Manifest,
    out: &mut Vec<Violation>,
) {
    for layer in &manifest.layers {
        for container in &layer.containers {
            // A container of planned slots only is intent. One declared
            // before any component (a profile's fresh shape, 0.4.2) is named
            // softly (0.4.9): it may be intent, or a typo that would leave
            // the container silently ungoverned.
            let fresh = container.components.is_empty();
            let all_planned = !fresh && container.components.iter().all(|c| c.planned);
            if !container.path.is_empty() && !root.join(&container.path).is_dir() && fresh {
                out.push(Violation {
                    check: "manifest-path".into(),
                    file: container.path.clone(),
                    line: 0,
                    message: format!(
                        "container `{}` declares path `{}/` that does not exist yet (no components either)",
                        container.name, container.path
                    ),
                    hint: "scaffold its first component there, or fix the path if it is a typo — until then nothing governs it".into(),
                    severity: Severity::Info,
                });
            } else if !container.path.is_empty()
                && !root.join(&container.path).is_dir()
                && !all_planned
            {
                out.push(Violation {
                    check: "manifest-path".into(),
                    file: container.path.clone(),
                    line: 0,
                    message: format!(
                        "container `{}` declares path `{}/` that does not exist",
                        container.name, container.path
                    ),
                    hint: "create the folder or update the manifest".into(),
                    severity: Severity::Error,
                });
            }
            for comp in &container.components {
                if comp.planned {
                    // a slot whose code now exists: say so once, softly
                    if !comp.path.is_empty() && root.join(&comp.path).exists() {
                        out.push(Violation {
                            check: "manifest-path".into(),
                            file: comp.path.clone(),
                            line: 0,
                            message: format!(
                                "planned slot `{}` now has its code at `{}`",
                                comp.name, comp.path
                            ),
                            hint: format!("mark it built: `harna slot fill {}`", comp.name),
                            severity: Severity::Info,
                        });
                    }
                    continue;
                }
                if comp.path.is_empty() || root.join(&comp.path).exists() {
                    continue;
                }
                let kind = match comp.kind {
                    crate::schema::model::manifest::ComponentKind::Primitive => "primitive",
                    crate::schema::model::manifest::ComponentKind::Compound => "compound",
                };
                out.push(Violation {
                    check: "manifest-path".into(),
                    file: comp.path.clone(),
                    line: 0,
                    message: format!(
                        "component `{}` ({}): path `{}` does not exist",
                        comp.name, kind, comp.path
                    ),
                    hint: "run `harna scaffold` or fix the manifest".into(),
                    severity: Severity::Error,
                });
            }
        }
    }
}

/// Layering order: `sequence` lists must name real members without duplicates;
/// unplaced members get an informational nudge.
pub fn check_sequences(
    manifest: &crate::schema::model::manifest::Manifest,
    out: &mut Vec<Violation>,
) {
    for layer in &manifest.layers {
        let container_names: Vec<&str> = layer.containers.iter().map(|c| c.name.as_str()).collect();
        check_one_sequence("layer", &layer.name, &layer.sequence, &container_names, out);

        for container in &layer.containers {
            let comp_names: Vec<&str> = container
                .components
                .iter()
                .map(|c| c.name.as_str())
                .collect();
            check_one_sequence(
                "container",
                &format!("{}/{}", layer.name, container.name),
                &container.sequence,
                &comp_names,
                out,
            );
        }
    }
}

fn check_one_sequence(
    scope_kind: &str,
    scope_name: &str,
    sequence: &[String],
    member_names: &[&str],
    out: &mut Vec<Violation>,
) {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for name in sequence {
        if !member_names.contains(&name.as_str()) {
            out.push(Violation {
                check: "manifest-sequence".into(),
                file: ".harna/manifest.toml".into(),
                line: 0,
                message: format!(
                    "{scope_kind} `{scope_name}`: sequence lists `{name}` but no such member exists"
                ),
                hint: "add the member, or fix the spelling in the sequence".into(),
                severity: Severity::Error,
            });
            continue;
        }
        if !seen.insert(name) {
            out.push(Violation {
                check: "manifest-sequence".into(),
                file: ".harna/manifest.toml".into(),
                line: 0,
                message: format!("{scope_kind} `{scope_name}`: sequence lists `{name}` twice"),
                hint: "remove the duplicate entry".into(),
                severity: Severity::Error,
            });
        }
    }
    if !sequence.is_empty() {
        let unlisted: Vec<&&str> = member_names
            .iter()
            .filter(|m| !sequence.iter().any(|s| s == **m))
            .collect();
        if !unlisted.is_empty() {
            out.push(Violation {
                check: "manifest-sequence".into(),
                file: ".harna/manifest.toml".into(),
                line: 0,
                message: format!(
                    "{scope_kind} `{scope_name}`: member(s) not in sequence: {}",
                    unlisted.iter().map(|s| **s).collect::<Vec<_>>().join(", ")
                ),
                hint: "add them to the sequence, or document why they are unordered".into(),
                severity: Severity::Info,
            });
        }
    }
}

/// Layer dependency declarations: a `depends_on` must name a real layer.
/// (Also rendered into deny.toml by `harna deny`.)
pub fn check_layer_depends(
    manifest: &crate::schema::model::manifest::Manifest,
    out: &mut Vec<Violation>,
) {
    let layer_names: Vec<&str> = manifest.layers.iter().map(|l| l.name.as_str()).collect();
    // A layer name is an address: two layers sharing one make every
    // depends_on naming it ambiguous (0.3.46, zed's two "gpui" crates).
    let mut seen = std::collections::BTreeSet::new();
    for name in &layer_names {
        if !seen.insert(*name) {
            out.push(Violation {
                check: "manifest-depends".into(),
                file: ".harna/manifest.toml".into(),
                line: 0,
                message: format!(
                    "two layers are named `{name}` — depends_on cannot tell them apart"
                ),
                hint: "rename one (`harna layer set` cannot; edit the name or re-adopt)".into(),
                severity: Severity::Error,
            });
        }
    }
    for layer in &manifest.layers {
        for dep in &layer.depends_on {
            if !layer_names.contains(&dep.as_str()) {
                out.push(Violation {
                    check: "manifest-depends".into(),
                    file: ".harna/manifest.toml".into(),
                    line: 0,
                    message: format!(
                        "layer `{}` depends_on `{dep}` but no such layer exists",
                        layer.name
                    ),
                    hint: "create the layer or fix the dependency declaration".into(),
                    severity: Severity::Error,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Component, ComponentKind, Container, Layer, Manifest};

    #[test]
    fn missing_component_paths_are_flagged() {
        let m = Manifest {
            test_kinds: Vec::new(),
            schema_version: 1,
            layers: vec![Layer {
                owners: Vec::new(),
                isolated: false,
                name: "l1".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                role: None,
                containers: vec![Container {
                    owners: Vec::new(),
                    escapes: Vec::new(),
                    owns: Vec::new(),
                    name: "c1".into(),
                    path: "nowhere".into(),
                    description: String::new(),
                    sequence: vec![],
                    provides: vec![],
                    role: None,
                    gateway: None,
                    connects: vec![],
                    components: vec![Component {
                        planned: false,
                        name: "gone".into(),
                        kind: ComponentKind::Primitive,
                        path: "nowhere/gone.rs".into(),
                        summary: String::new(),
                        tests: vec![],
                        provides: vec![],
                        children: vec![],
                    }],
                }],
            }],
        };
        let tmp = std::env::temp_dir();
        let mut v = Vec::new();
        check_paths_exist(&tmp, &m, &mut v);
        assert_eq!(v.len(), 2, "container dir + component path both missing");
        assert!(v.iter().all(|x| x.check == "manifest-path"));
    }

    #[test]
    fn sequence_entries_are_checked() {
        let m = Manifest {
            test_kinds: Vec::new(),
            schema_version: 1,
            layers: vec![Layer {
                owners: Vec::new(),
                isolated: false,
                name: "l1".into(),
                description: String::new(),
                sequence: vec!["c1".into(), "c1".into(), "ghost".into()],
                depends_on: vec![],
                role: None,
                containers: vec![Container {
                    owners: Vec::new(),
                    escapes: Vec::new(),
                    owns: Vec::new(),
                    name: "c1".into(),
                    path: String::new(),
                    description: String::new(),
                    sequence: vec!["comp-x".into()],
                    provides: vec![],
                    role: None,
                    gateway: None,
                    connects: vec![],
                    components: vec![
                        Component {
                            planned: false,
                            name: "comp-x".into(),
                            kind: ComponentKind::Primitive,
                            path: String::new(),
                            summary: String::new(),
                            tests: vec![],
                            provides: vec![],
                            children: vec![],
                        },
                        Component {
                            planned: false,
                            name: "comp-y".into(),
                            kind: ComponentKind::Primitive,
                            path: String::new(),
                            summary: String::new(),
                            tests: vec![],
                            provides: vec![],
                            children: vec![],
                        },
                    ],
                }],
            }],
        };
        let mut v = Vec::new();
        check_sequences(&m, &mut v);
        assert!(v
            .iter()
            .any(|x| x.severity == Severity::Error && x.message.contains("twice")));
        assert!(v
            .iter()
            .any(|x| x.severity == Severity::Error && x.message.contains("ghost")));
        assert!(v
            .iter()
            .any(|x| x.severity == Severity::Info && x.message.contains("not in sequence")));
    }

    #[test]
    fn unknown_layer_dependencies_are_flagged() {
        let m = Manifest {
            test_kinds: Vec::new(),
            schema_version: 1,
            layers: vec![Layer {
                owners: Vec::new(),
                isolated: false,
                name: "l1".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec!["missing-layer".into()],
                containers: vec![],
                role: None,
            }],
        };
        let mut v = Vec::new();
        check_layer_depends(&m, &mut v);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].check, "manifest-depends");
    }

    #[test]
    fn duplicate_layer_names_are_named() {
        let layer = |n: &str| Layer {
            name: n.into(),
            ..Default::default()
        };
        let m = Manifest {
            test_kinds: Vec::new(),
            schema_version: 1,
            layers: vec![layer("gpui"), layer("core"), layer("gpui")],
        };
        let mut v = Vec::new();
        check_layer_depends(&m, &mut v);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("two layers are named `gpui`"));
    }
}
