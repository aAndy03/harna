//! Validation: compare the codebase against the manifest and rules.
//!
//! These checks turn "preventing drift" from a suggestion into enforcement.
//! Every violation names its rule, file and line — an agent can fix what it
//! can locate.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::store::Store;

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
}

impl Violation {
    /// Token-efficient rendering for agent consumption.
    pub fn render(&self) -> String {
        format!(
            "[{}] {}:{} — {}\n  fix: {}",
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

        // Layer-boundary checks only apply when the manifest defines layers.
        if !manifest.is_empty() {
            for layer in &manifest.layers {
                let owned = layer_owned_file(layer, file_rel);
                if owned {
                    let others: Vec<String> = manifest
                        .layers
                        .iter()
                        .filter(|l| l.name != layer.name)
                        .map(|l| l.name.clone())
                        .collect();
                    check_layer_boundary_crossing(&layer.name, &others, &src, file_rel, &mut violations);
                }
            }
        }
    }

    // Deduplicate: a file can belong to several declared layer scopes.
    violations.dedup();

    check_manifest_paths_exist(root, &manifest, &mut violations);

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
    for (i, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
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
            });
        }
    }
}

/// A file belongs to a layer when its path starts with the layer's
/// normalized scope (`my-layer` -> `src/my_layer/` or `src/my-layer/`).
fn layer_owned_file(layer: &crate::manifest::Layer, file: &str) -> bool {
    let norm = file.replace('-', "_").replace('\\', "/");
    let layer_secret = layer.name.replace('-', "_");
    norm.starts_with(&format!("src/{}", layer_secret))
        || norm.starts_with(&format!("src/{}", layer.name))
}

/// Manifest drift: components whose `path` no longer exists on disk.
fn check_manifest_paths_exist(
    root: &Path,
    manifest: &crate::manifest::Manifest,
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
                });
            }
            for comp in &container.components {
                if comp.path.is_empty() || root.join(&comp.path).exists() {
                    continue;
                }
                let kind = match comp.kind {
                    crate::manifest::ComponentKind::Primitive => "primitive",
                    crate::manifest::ComponentKind::Compound => "compound",
                };
                out.push(Violation {
                    check: "manifest-path".into(),
                    file: comp.path.clone(),
                    line: 0,
                    message: format!("component `{}` ({}): path `{}` does not exist", comp.name, kind, comp.path),
                    hint: "run `harna scaffold` or fix the manifest".into(),
                });
            }
        }
    }
}

/// Layering: `use crate::other_layer::` inside a layer's directory is a
/// forbidden cross-layer import (engines talk through transiters, not imports).
fn check_layer_boundary_crossing(
    own_layer: &str,
    other_layers: &[String],
    src: &str,
    file: &str,
    out: &mut Vec<Violation>,
) {
    for (i, line) in src.lines().enumerate() {
        let t = line.trim();
        let rest = match t.strip_prefix("use crate::") {
            Some(r) => r,
            None => continue,
        };
        for other in other_layers {
            let other_norm = other.replace('-', "_");
            if other_norm.is_empty() {
                continue;
            }
            if rest.starts_with(&other_norm) {
                let layer_secret = own_layer.replace('-', "_");
                if layer_secret == other_norm {
                    continue;
                }
                out.push(Violation {
                    check: "layer-boundary".into(),
                    file: file.into(),
                    line: i + 1,
                    message: format!(
                        "cross-layer import: `{own_layer}` reaches into `{other}`"
                    ),
                    hint: "route the interaction through a transiter engine".into(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn cross_layer_import_is_flagged() {
        let mut v = Vec::new();
        check_layer_boundary_crossing(
            "api",
            &["engine-billing".to_string()],
            "use crate::engine_billing::billing::charge;\nfn main() {}",
            "src/api/handlers.rs",
            &mut v,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].check, "layer-boundary");
    }

    #[test]
    fn same_layer_import_is_allowed() {
        let mut v = Vec::new();
        check_layer_boundary_crossing(
            "api",
            &["api".to_string()],
            "use crate::api::users::list;\nfn main() {}",
            "src/api/handlers.rs",
            &mut v,
        );
        assert!(v.is_empty());
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

        let store = crate::store::Store::open(&root).unwrap();
        // Only the clean file is checked: no violations.
        let scoped = run_on(&root, &store, 100, Some(vec!["src/api/clean.rs".into()])).unwrap();
        assert!(scoped.is_empty());
        // Full scan finds the violation.
        let full = run(&root, &store, 100).unwrap();
        assert!(full.iter().any(|v| v.check == "no-hardcoded-urls"));
    }
}
