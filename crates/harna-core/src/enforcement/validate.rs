//! Validation: compare the codebase against the manifest and rules.
//!
//! These checks turn "preventing drift" from a suggestion into enforcement.
//! Every violation names its rule, file and line — an agent can fix what it
//! can locate.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::schema::store::Store;

/// One conformance violation found in the codebase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    /// Which check fired, e.g. "no-hardcoded-urls".
    pub check: String,
    /// File path relative to repo root.
    pub file: String,
    /// 1-based line number, or 0 when not line-specific.
    pub line: usize,
    /// Human/agent-readable description.
    pub message: String,
    /// Suggested fix, rendered for the agent.
    pub hint: String,
    /// Error violations gate commits/CI (exit code 2); info ones advise only.
    #[serde(default)]
    pub severity: Severity,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Error,
    Info,
}

impl Violation {
    /// Token-efficient rendering for agent consumption.
    pub fn render(&self) -> String {
        let marker = match self.severity {
            Severity::Error => "!",
            Severity::Info => "?",
        };
        format!(
            "[{marker}{}] {}:{} — {}\n  fix: {}",
            self.check,
            self.file,
            if self.line > 0 { self.line.to_string() } else { "-".into() },
            self.message,
            self.hint
        )
    }
}

const SKIP_DIRS: &[&str] = &[".harna", "target", "node_modules", ".git"];

/// Run all checks over the whole repo.
pub fn run(root: &Path, store: &Store, max_files: usize) -> anyhow::Result<Vec<Violation>> {
    run_on(root, store, max_files, None)
}

/// Run checks over only the given `.rs` files (relative paths) — the
/// enforcement loop for pre-commit hooks and "validate after you edit".
///
/// Manifest-level checks (broken component paths) always run: they are cheap
/// and read-only. File-level checks (URLs, layer boundaries) run on the
/// provided files only.
pub fn run_on(
    root: &Path,
    store: &Store,
    max_files: usize,
    files: Option<Vec<String>>,
) -> anyhow::Result<Vec<Violation>> {
    let mut violations = Vec::new();
    let mut srcs: Vec<(String, String)> = Vec::new();
    let manifest = store.load_manifest().unwrap_or_default();

    let rust_files: Vec<String> = match files {
        Some(f) => f.into_iter().take(max_files).collect(),
        None => {
            let mut v = Vec::new();
            collect_rust_files(root, &mut v, max_files)?;
            v
        }
    };

    for file_rel in &rust_files {
        let abs = root.join(file_rel);
        let src = match fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => continue,
        };
        check_hardcoded_urls(&src, file_rel, &mut violations);
        srcs.push((file_rel.clone(), src));
    }

    if !manifest.is_empty() {
        check_layer_boundaries_real(root, &manifest, &srcs, &mut violations);
    }

    // Deduplicate: a file can belong to several declared layer scopes.
    violations.dedup();

    check_manifest_paths_exist(root, &manifest, &mut violations);
    check_sequences(&manifest, &mut violations);
    check_layer_depends(&manifest, &mut violations);
    check_parity_open(store, &mut violations);
    check_sequences(&manifest, &mut violations);

    violations.sort_by(|a, b| (&a.file, a.line, &a.check).cmp(&(&b.file, b.line, &b.check)));
    Ok(violations)
}

fn collect_rust_files(
    root: &Path,
    out: &mut Vec<String>,
    max_files: usize,
) -> anyhow::Result<()> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if SKIP_DIRS.contains(&name) {
                    continue;
                }
                stack.push(path);
            } else if out.len() < max_files
                && path.extension().and_then(|e| e.to_str()) == Some("rs")
            {
                out.push(
                    path.strip_prefix(root)
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default(),
                );
            }
        }
    }
    Ok(())
}

/// Parse `git status --porcelain` output into relative `.rs` file paths.
/// Handles staged (`M  `), unstaged (` M `), untracked (`?? `) and rename
/// (`R  old -> new`) entries; keeps the new side of renames.
fn parse_git_status(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let mut path = line[3..].to_string();
            if let Some((_, new)) = path.split_once(" -> ") {
                path = new.to_string();
            }
            let path = path.trim().replace('\\', "/");
            if path.ends_with(".rs") {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}

/// `.rs` files with uncommitted changes — the input for `validate --changed`.
pub fn changed_rust_files(root: &Path) -> anyhow::Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .map_err(|e| anyhow::anyhow!("running git failed: {e} — `--changed` requires a git repository"))?;
    if !output.status.success() {
        anyhow::bail!(
            "git status failed — `--changed` requires a git repository"
        );
    }
    Ok(parse_git_status(&String::from_utf8_lossy(&output.stdout)))
}

/// Rule: no `http://`/`https://` literals hardwired in source.
/// URLs belong in an `Endpoint` constants module so they can be swapped per env.
fn check_hardcoded_urls(src: &str, file: &str, out: &mut Vec<Violation>) {
    // File-level carve-out: the endpoint/constants module may define them.
    let file_lower = file.to_lowercase();
    if file_lower.contains("endpoint") || file_lower.contains("constants") {
        return;
    }
    // Test code may legitimately embed URLs in fixtures.
    let mut in_test_code = false;
    for (i, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        // Test modules are declared near the end of Rust files; once we are
        // inside one, every remaining line is test code.
        if !in_test_code
            && (trimmed.contains("#[cfg(test)]")
                || trimmed.starts_with("mod tests")
                || trimmed.starts_with("#[test]"))
        {
            in_test_code = true;
        }
        if in_test_code || trimmed.starts_with("//") {
            continue;
        }
        // Only count URLs inside string literals (heuristic: a quote on the line).
        if (line.contains("http://") || line.contains("https://")) && line.contains('"') {
            out.push(Violation {
                check: "no-hardcoded-urls".into(),
                file: file.into(),
                line: i + 1,
                message: "hardcoded URL in source".into(),
                hint: "move the URL into the Endpoint constants module and reference it".into(),
                severity: Severity::Error,
            });
        }
    }
}

/// Layering order: `sequence` lists must name real members without duplicates,
/// and scopes with several members but no sequence get an informational nudge.
fn check_sequences(
    manifest: &crate::schema::manifest::Manifest,
    out: &mut Vec<Violation>,
) {
    for layer in &manifest.layers {
        let container_names: Vec<&str> =
            layer.containers.iter().map(|c| c.name.as_str()).collect();
        check_one_sequence(
            "layer",
            &layer.name,
            &layer.sequence,
            &container_names,
            out,
        );

        for container in &layer.containers {
            let comp_names: Vec<&str> =
                container.components.iter().map(|c| c.name.as_str()).collect();
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
    let mut seen = HashSet::new();
    for name in sequence {
        if !member_names.contains(&name.as_str()) {
            out.push(Violation {
                check: "manifest-sequence".into(),
                file: ".harna/manifest.toml".into(),
                line: 0,
                message: format!("{scope_kind} `{scope_name}`: sequence lists `{name}` but no such member exists"),
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
/// Layering on real dependency analysis: each internal import is resolved to
/// its target crate + module stem, matched against manifest containers, and
/// compared to the importer's allowed layer set (own layers + declared
/// `depends_on`). No name-prefix guessing.
fn check_layer_boundaries_real(
    root: &Path,
    manifest: &crate::schema::manifest::Manifest,
    srcs: &[(String, String)],
    out: &mut Vec<Violation>,
) {
    let labels = container_labels(root, manifest);
    let crate_names = crate::search::digest::workspace_crate_names(root);
    let layer_depends: std::collections::HashMap<&str, Vec<&str>> = manifest
        .layers
        .iter()
        .map(|l| {
            (
                l.name.as_str(),
                l.depends_on.iter().map(|s| s.as_str()).collect(),
            )
        })
        .collect();

    for (file, src) in srcs {
        let home: Vec<&ContainerLabel> = labels
            .iter()
            .filter(|l| file.starts_with(&format!("{}/", l.container_path)))
            .collect();
        if home.is_empty() {
            continue; // ungoverned file
        }
        let home_layers: Vec<&str> = home.iter().map(|l| l.layer.as_str()).collect();
        let home_crate_dir = home[0].crate_dir.as_str();
        let importer_stem = crate::search::digest::crate_of(file).1;

        let deps = crate::search::digest::extract_deps(src, &crate_names);
        for dep in deps {
            let Some(target) =
                crate::search::digest::resolve_dep(&dep, &importer_stem, &crate_names)
            else {
                continue;
            };
            let target_labels: Vec<&ContainerLabel> = labels
                .iter()
                .filter(|l| match target.external.as_deref() {
                    Some(crate_name) => l.crate_pkg == crate_name,
                    None => l.crate_dir == home_crate_dir,
                })
                .filter(|l| stem_in_container(&target.stem, &l.rel_prefix))
                .collect();
            if target_labels.is_empty() {
                continue; // target outside the manifest's reach
            }
            let mut allowed = home_layers.clone();
            for hl in &home_layers {
                if let Some(deps) = layer_depends.get(*hl) {
                    allowed.extend(deps.iter().copied());
                }
            }
            let reached: Vec<&str> = target_labels
                .iter()
                .map(|l| l.layer.as_str())
                .filter(|tl| !allowed.contains(tl))
                .collect();
            if reached.is_empty() {
                continue;
            }
            out.push(Violation {
                check: "layer-boundary".into(),
                file: file.clone(),
                line: find_line(src, &dep),
                message: format!(
                    "cross-layer import `{dep}` reaches layer(s) {} not in {}'s allowed set",
                    reached.join(", "),
                    home_layers.join("/")
                ),
                hint: "declare depends_on on the importing layer, or route via a transiter"
                    .into(),
                severity: Severity::Error,
            });
        }
    }
}

/// Does module stem `stem` lie inside container prefix `rel_prefix`?
fn stem_in_container(stem: &str, rel_prefix: &str) -> bool {
    rel_prefix.is_empty()
        || stem == rel_prefix
        || stem.starts_with(&format!("{rel_prefix}/"))
        || rel_prefix.starts_with(&format!("{stem}/"))
}

fn find_line(src: &str, needle: &str) -> usize {
    src.lines()
        .enumerate()
        .find(|(_, l)| l.contains(needle))
        .map(|(i, _)| i + 1)
        .unwrap_or(0)
}

struct ContainerLabel {
    layer: String,
    /// Directory of the owning crate, e.g. "crates/harna-core" ("." for the root).
    crate_dir: String,
    /// Package name of that crate.
    crate_pkg: String,
    /// Crate-relative prefix of the container path, e.g. "schema".
    rel_prefix: String,
    container_path: String,
}

fn container_labels(
    root: &Path,
    manifest: &crate::schema::manifest::Manifest,
) -> Vec<ContainerLabel> {
    let mut labels = Vec::new();
    for layer in &manifest.layers {
        for c in &layer.containers {
            if c.path.is_empty() {
                continue;
            }
            let (crate_dir, rel_prefix) = match c.path.split_once("/src/") {
                Some((before, tail)) => (before.to_string(), tail.to_string()),
                None if c.path.starts_with("src/") => (".".to_string(), c.path[4..].to_string()),
                _ => continue,
            };
            let cargo = root.join(&crate_dir).join("Cargo.toml");
            let Ok(src) = fs::read_to_string(&cargo) else { continue };
            let Ok(value) = toml::from_str::<toml::Value>(&src) else { continue };
            let Some(pkg) = value
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
            else {
                continue;
            };
            labels.push(ContainerLabel {
                layer: layer.name.clone(),
                crate_dir,
                crate_pkg: pkg.to_string(),
                rel_prefix: rel_prefix.trim_end_matches('/').to_string(),
                container_path: c.path.clone(),
            });
        }
    }
    labels
}

/// Manifest drift: components whose `path` no longer exists on disk.
fn check_manifest_paths_exist(
    root: &Path,
    manifest: &crate::schema::manifest::Manifest,
    out: &mut Vec<Violation>,
) {
    for layer in &manifest.layers {
        for container in &layer.containers {
            if !container.path.is_empty() && !root.join(&container.path).is_dir() {
                out.push(Violation {
                    check: "manifest-path".into(),
                    file: container.path.clone(),
                    line: 0,
                    message: format!("container `{}` declares path `{}/` that does not exist", container.name, container.path),
                    hint: "create the folder or update the manifest".into(),
                    severity: Severity::Error,
                });
            }
            for comp in &container.components {
                if comp.path.is_empty() || root.join(&comp.path).exists() {
                    continue;
                }
                let kind = match comp.kind {
                    crate::schema::manifest::ComponentKind::Primitive => "primitive",
                    crate::schema::manifest::ComponentKind::Compound => "compound",
                };
                out.push(Violation {
                    check: "manifest-path".into(),
                    file: comp.path.clone(),
                    line: 0,
                    message: format!("component `{}` ({}): path `{}` does not exist", comp.name, kind, comp.path),
                    hint: "run `harna scaffold` or fix the manifest".into(),
                    severity: Severity::Error,
                });
            }
        }
    }
}

/// Parity gate: unresolved parity debts block conformance. An open target
/// means a structural change propagated to a module that has not yet been
/// acknowledged (`ack` was done), recorded (`noop`, nothing needed), or
/// verified by harna (`auto`, surface unchanged).
fn check_parity_open(store: &Store, out: &mut Vec<Violation>) {
    let events = match crate::enforcement::parity::list_events(store) {
        Ok(e) => e,
        // No parity directory / unreadable ledger: skip silently (bootstrap).
        Err(_) => return,
    };
    for ev in events {
        let open = ev.open_targets().len();
        if open == 0 {
            continue;
        }
        out.push(Violation {
            check: "parity-open".into(),
            file: format!(".harna/parity/{}.toml", ev.id),
            line: 0,
            message: format!(
                "event `{}` (gen {}): {} unresolved parity target(s)",
                ev.id, ev.generation, open
            ),
            hint: "ack or noop each open target (harna_parity_resolve), or land the change in those modules".into(),
            severity: Severity::Error,
        });
    }
}

/// Layer dependency declarations: a `depends_on` must name a real layer.
/// (Also rendered into deny.toml by `harna deny`.)
fn check_layer_depends(
    manifest: &crate::schema::manifest::Manifest,
    out: &mut Vec<Violation>,
) {
    let layer_names: Vec<&str> = manifest.layers.iter().map(|l| l.name.as_str()).collect();
    for layer in &manifest.layers {
        for dep in &layer.depends_on {
            if !layer_names.contains(&dep.as_str()) {
                out.push(Violation {
                    check: "manifest-depends".into(),
                    file: ".harna/manifest.toml".into(),
                    line: 0,
                    message: format!("layer `{}` depends_on `{dep}` but no such layer exists", layer.name),
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

    fn write_min_crate(root: &Path, dir: &str, pkg: &str) {
        let cargo = root.join(dir).join("Cargo.toml");
        std::fs::create_dir_all(cargo.parent().unwrap()).unwrap();
        std::fs::write(&cargo, format!("[package]\nname = \"{pkg}\"\nversion = \"0.1.0\"\n")).unwrap();
    }

    fn boundary_fixture(tag: &str) -> (std::path::PathBuf, crate::schema::manifest::Manifest) {
        use crate::schema::manifest::{Container, Layer, Manifest};

        let root = std::env::temp_dir().join(format!("harna-boundary-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        write_min_crate(&root, ".", "t");
        write_min_crate(&root, "crates/t", "t");

        let manifest = Manifest {
            schema_version: 1,
            layers: vec![
                Layer {
                    name: "api".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    containers: vec![Container {
                        name: "handlers".into(),
                        path: "crates/t/src/api".into(),
                        sequence: vec![],
                        components: vec![],
                    }],
                },
                Layer {
                    name: "engine".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    containers: vec![Container {
                        name: "core".into(),
                        path: "crates/t/src/engine".into(),
                        sequence: vec![],
                        components: vec![],
                    }],
                },
            ],
        };
        (root, manifest)
    }

    #[test]
    fn real_boundary_flags_undeclared_cross_layer_import() {
        use crate::schema::manifest::Manifest;

        let (root, mut manifest) = boundary_fixture("flags");
        std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
        std::fs::create_dir_all(root.join("crates/t/src/engine")).unwrap();
        std::fs::write(
            root.join("crates/t/src/api/h.rs"),
            "use crate::engine::billing::charge;\nfn main() {}",
        )
        .unwrap();
        let srcs = vec![(
            "crates/t/src/api/h.rs".to_string(),
            std::fs::read_to_string(root.join("crates/t/src/api/h.rs")).unwrap(),
        )];

        let mut v = Vec::new();
        check_layer_boundaries_real(&root, &manifest, &srcs, &mut v);
        assert_eq!(v.len(), 1, "undeclared cross-layer import is an error");
        assert_eq!(v[0].check, "layer-boundary");
        assert_eq!(v[0].line, 1);
        assert!(v[0].message.contains("engine"));

        // Declaring the dependency makes the same import legal.
        manifest.layers[0].depends_on = vec!["engine".into()];
        let mut v2 = Vec::new();
        check_layer_boundaries_real(&root, &manifest, &srcs, &mut v2);
        assert_eq!(v2.len(), 1 - 1);
    }

    #[test]
    fn real_boundary_allows_same_layer_and_ungoverned_stems() {
        use crate::schema::manifest::Manifest;

        let (root, manifest) = boundary_fixture("allowed");
        std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
        std::fs::create_dir_all(root.join("crates/t/src/engine")).unwrap();

        // Same-container import + import of a stem no container governs
        // (crate-root symbol `T` is ungoverned -> skipped).
        std::fs::write(
            root.join("crates/t/src/api/h.rs"),
            "use crate::api::helpers;\nuse crate::T;\nfn main() {}",
        )
        .unwrap();
        let srcs = vec![(
            "crates/t/src/api/h.rs".to_string(),
            std::fs::read_to_string(root.join("crates/t/src/api/h.rs")).unwrap(),
        )];

        let mut v = Vec::new();
        check_layer_boundaries_real(&root, &manifest, &srcs, &mut v);
        assert!(v.is_empty(), "same-layer and ungoverned imports allowed");
    }

    #[test]
    fn hardcoded_urls_flagged_but_not_in_constants_module() {
        let mut v = Vec::new();
        check_hardcoded_urls(
            "fn main() {\n    fetch(\"https://api.example.com/users\");\n}",
            "src/api/users.rs",
            &mut v,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].check, "no-hardcoded-urls");
        assert_eq!(v[0].line, 2);

        let mut v2 = Vec::new();
        check_hardcoded_urls(
            "pub const USERS: &str = \"https://api.example.com/users\";",
            "src/constants/endpoints.rs",
            &mut v2,
        );
        assert!(v2.is_empty());
    }

    #[test]
    fn comments_and_url_free_lines_are_ignored() {
        let mut v = Vec::new();
        check_hardcoded_urls("// see https://example.com/docs", "src/x.rs", &mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn test_code_may_embed_urls() {
        // Test modules declare themselves near the end of Rust files;
        // everything from that marker on is fixture data, not shipped code.
        let src = "pub fn run() {}\n\n#[cfg(test)]\nmod tests {\n    \
const FIXTURE: &str = \"https://test.example.com/\";\n}";
        let mut v = Vec::new();
        check_hardcoded_urls(src, "src/run.rs", &mut v);
        assert!(v.is_empty());

        // Production code before the marker is still checked.
        let src2 = "pub fn a() { fetch(\"https://prod.example.com\"); }\n\
#[cfg(test)]\nmod tests { /* fixture with https://fixture.example.com */ }";
        let mut v2 = Vec::new();
        check_hardcoded_urls(src2, "src/a.rs", &mut v2);
        assert_eq!(v2.len(), 1);
        assert_eq!(v2[0].line, 1);
    }

    #[test]
    fn git_status_parsing_handles_all_states() {
        let porcelain = "\
M  src/api/handlers.rs
 M src/engine/scanner.rs
?? src/new_module.rs
R  src/old.rs -> src/renamed.rs
 M README.md
A  src/added.rs
MM src/modified_twice.rs
";
        let files = parse_git_status(porcelain);
        assert!(files.contains(&"src/api/handlers.rs".to_string()));
        assert!(files.contains(&"src/engine/scanner.rs".to_string()));
        assert!(files.contains(&"src/new_module.rs".to_string()));
        // renames keep the new path
        assert!(files.contains(&"src/renamed.rs".to_string()));
        assert!(!files.contains(&"src/old.rs".to_string()));
        // non-rust files are filtered out
        assert!(!files.contains(&"README.md".to_string()));
        assert_eq!(files.len(), 6);
    }

    #[test]
    fn empty_status_gives_no_files() {
        assert!(parse_git_status("").is_empty());
    }

    #[test]
    fn run_on_scopes_file_checks_to_listed_files() {
        // A standalone file check through run_on's per-file path.
        let root = std::env::temp_dir().join(format!("harna-runon-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".harna")).unwrap();
        std::fs::write(root.join(".harna/rules.toml"), "schema_version = 1\n").unwrap();
        std::fs::create_dir_all(root.join("src/api")).unwrap();
        std::fs::write(
            root.join("src/api/handlers.rs"),
            "fn bad() { fetch(\"https://api.example.com\"); }",
        )
        .unwrap();
        std::fs::write(
            root.join("src/api/clean.rs"),
            "pub fn good() {}",
        )
        .unwrap();

        let store = crate::schema::store::Store::open(&root).unwrap();
        // Only the clean file is checked: no violations.
        let scoped = run_on(&root, &store, 100, Some(vec!["src/api/clean.rs".into()])).unwrap();
        assert!(scoped.is_empty());
        // Full scan finds the violation.
        let full = run(&root, &store, 100).unwrap();
        assert!(full.iter().any(|v| v.check == "no-hardcoded-urls"));
    }
}
