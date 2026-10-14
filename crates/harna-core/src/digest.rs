//! Digest: a compressed, queryable map of the codebase.
//!
//! The agent queries the digest instead of rescanning thousands of files.
//! Only signatures and structure are stored — never file bodies — so the
//! map stays small enough to inline into a prompt or diff in a PR.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Compressed code map (`.harna/digest.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Digest {
    pub generated_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<Module>,
}

/// One source module (file or folder) reduced to its essentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    /// Path relative to repo root.
    pub path: String,
    /// Language hint, e.g. "rust".
    pub lang: String,
    /// Public signatures (functions, structs, traits) — no bodies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signatures: Vec<String>,
    /// Direct internal dependencies (module paths).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
}

impl Digest {
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    /// Total signature count across modules — a rough "size of the API".
    pub fn signature_count(&self) -> usize {
        self.modules.iter().map(|m| m.signatures.len()).sum()
    }

    /// Render a token-efficient overview for agents.
    pub fn render_overview(&self, max_modules: usize) -> String {
        let mut out = format!(
            "codebase map: {} modules, {} signatures (generated {})\n",
            self.module_count(),
            self.signature_count(),
            self.generated_at
        );
        for m in self.modules.iter().take(max_modules) {
            out.push_str(&format!("- {} ({})\n", m.path, m.lang));
            for sig in m.signatures.iter().take(5) {
                out.push_str(&format!("    {}\n", sig));
            }
            if m.signatures.len() > 5 {
                out.push_str(&format!("    … +{} more\n", m.signatures.len() - 5));
            }
        }
        if self.modules.len() > max_modules {
            out.push_str(&format!("… +{} more modules\n", self.modules.len() - max_modules));
        }
        out
    }
}

/// Scan a repository and build a digest.
///
/// MVP heuristic: for each `.rs` file, capture `fn`, `struct`, `enum` and
/// `trait` declaration lines as signatures. Depth-capped to keep scans fast.
pub fn build(root: &Path, max_files: usize) -> anyhow::Result<Digest> {
    let mut modules = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0usize;

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == ".harna" || name == "target" || name == "node_modules" {
                    continue;
                }
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                if seen >= max_files {
                    return Ok(Digest { generated_at: now(), modules });
                }
                seen += 1;
                modules.push(scan_rust_file(root, &path));
            }
        }
    }

    Ok(Digest { generated_at: now(), modules })
}

fn scan_rust_file(root: &Path, path: &Path) -> Module {
    let rel = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let mut signatures = Vec::new();
    if let Ok(src) = fs::read_to_string(path) {
        for line in src.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("pub trait ")
            {
                signatures.push(trimmed.trim_end().to_string());
            }
        }
    }
    Module { path: rel, lang: "rust".into(), signatures, deps: Vec::new() }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_overview_with_truncation() {
        let d = Digest {
            generated_at: "now".into(),
            modules: vec![Module {
                path: "src/lib.rs".into(),
                lang: "rust".into(),
                signatures: (0..9).map(|i| format!("pub fn f{i}()")).collect(),
                deps: vec![],
            }],
        };
        let text = d.render_overview(5);
        assert!(text.contains("src/lib.rs"));
        assert!(text.contains("+4 more")); // 9 sigs, first 5 shown
    }
}
