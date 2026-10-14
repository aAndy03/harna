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
/// `trait` declaration lines as signatures, and internal `use` statements as
/// the module's dependency list (only `crate::`/`super::`/`self::` and
/// workspace-crate imports — never std or external crates). Depth-capped to
/// keep scans fast.
pub fn build(root: &Path, max_files: usize) -> anyhow::Result<Digest> {
    let mut modules = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0usize;
    let crate_names = workspace_crate_names(root);

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
                modules.push(scan_rust_file(root, &path, &crate_names));
            }
        }
    }

    Ok(Digest { generated_at: now(), modules })
}

/// Names of crates declared in the workspace — `use <name>::`
/// statements in member crates are internal deps, not external ones.
/// Public so the parity ledger can resolve cross-crate import edges.
pub fn workspace_crate_names(root: &Path) -> Vec<String> {
    let Ok(manifest) = fs::read_to_string(root.join("Cargo.toml")) else {
        return Vec::new();
    };
    let Ok(value) = toml::from_str::<toml::Value>(&manifest) else {
        return Vec::new();
    };
    let Some(members) = value
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for member in members {
        let Some(s) = member.as_str() else { continue };
        collect_member_crate_names(root, s, &mut names);
    }
    names
}

/// Expand one workspace member (dir path or glob like `crates/*`) to the
/// package name(s) declared in its Cargo.toml.
fn collect_member_crate_names(root: &Path, member: &str, out: &mut Vec<String>) {
    if member.contains('*') {
        let (before, after) = member
            .split_once('*')
            .map(|(b, a)| (b.to_string(), a.to_string()))
            .unwrap_or_else(|| (member.into(), String::new()));
        if let Ok(dirs) = fs::read_dir(root.join(&before)) {
            for entry in dirs.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let rest = after.trim_start_matches(['/', '\\']);
                let cargo = entry.path().join(rest).join("Cargo.toml");
                push_crate_name(&cargo, out);
            }
        }
    } else {
        push_crate_name(&root.join(member).join("Cargo.toml"), out);
    }
}

fn push_crate_name(cargo: &Path, out: &mut Vec<String>) {
    if let Ok(src) = fs::read_to_string(cargo) {
        if let Ok(v) = toml::from_str::<toml::Value>(&src) {
            if let Some(name) = v
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
            {
                out.push(name.to_string());
            }
        }
    }
}

/// Extract a module's internal dependency list from its `use` statements.
/// Dependency ids keep the raw path shape: `crate::schema::store`,
/// `super::x`, or `harna_core::schema::store`. Test sections are skipped
/// so the graph reflects shipped code, not fixtures.
fn extract_deps(src: &str, crate_names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("mod tests") {
            break;
        }
        let raw = trimmed.trim_end_matches(';');
        let Some(rest) = raw.strip_prefix("use ") else { continue };
        // `a::b::{x, y}` -> "a::b" (head before the brace group is a module path)
        let head = rest
            .split('{')
            .next()
            .unwrap_or(rest)
            .trim()
            .trim_end_matches("::")
            .to_string();
        if head.is_empty() || !head.contains("::") {
            continue;
        }
        let rooted = head.starts_with("crate::")
            || head.starts_with("super::")
            || head.starts_with("self::")
            || crate_names
                .iter()
                .any(|n| head == *n || head.starts_with(&format!("{n}::")));
        if rooted && !out.contains(&head) {
            out.push(head);
        }
    }
    out
}

fn scan_rust_file(root: &Path, path: &Path, crate_names: &[String]) -> Module {
    let rel = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let mut signatures = Vec::new();
    let mut deps: Vec<String> = Vec::new();
    if let Ok(src) = fs::read_to_string(path) {
        // Test sections are fixtures, not shipped surface: their items must
        // not appear as signatures (same rule as validate and deps).
        let mut in_test_code = false;
        for line in src.lines() {
            let trimmed = line.trim();
            if !in_test_code
                && (trimmed.starts_with("#[cfg(test)]")
                    || trimmed.starts_with("mod tests")
                    || trimmed.starts_with("#[test]"))
            {
                in_test_code = true;
            }
            if in_test_code {
                continue;
            }
            let pub_rest = line.trim_start().strip_prefix("pub ").unwrap_or("");
            if pub_rest.starts_with("fn ")
                || pub_rest.starts_with("struct ")
                || pub_rest.starts_with("enum ")
                || pub_rest.starts_with("trait ")
            {
                signatures.push(trimmed.trim_end().to_string());
            }
        }
        deps = extract_deps(&src, crate_names);
    }
    Module { path: rel, lang: "rust".into(), signatures, deps }
}


fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_deps_internal_only() {
        let src = "\
use std::fs;
use serde::Serialize;
use crate::schema::store::Store;
use crate::schema::store::{Store, StoreOpen};
use super::digest::Digest;
use self::helpers::x;
use harna_core::schema::manifest::Manifest;
pub fn main() {}
";
        let names = vec!["harna_core".to_string()];
        let deps = extract_deps(src, &names);
        assert!(deps.contains(&"crate::schema::store::Store".to_string()));
        // brace-group import collapses to its head module path
        assert!(deps.contains(&"crate::schema::store".to_string()));
        assert!(deps.contains(&"super::digest::Digest".to_string()));
        assert!(deps.contains(&"self::helpers::x".to_string()));
        assert!(deps.contains(&"harna_core::schema::manifest::Manifest".to_string()));
        // std and external crates are not internal deps
        assert!(!deps.contains(&"std::fs".to_string()));
        assert!(!deps.contains(&"serde::Serialize".to_string()));
    }

    #[test]
    fn extract_deps_skips_test_sections() {
        let src = "use crate::schema::store::Store;\n\
#[cfg(test)]\nmod tests {\n    use crate::fixture::OnlyFixture;\n}";
        let deps = extract_deps(src, &[]);
        assert!(deps.contains(&"crate::schema::store::Store".to_string()));
        assert!(!deps.contains(&"crate::fixture::OnlyFixture".to_string()));
    }

    #[test]
    fn test_fixtures_do_not_pollute_signatures() {
        // scan_rust_file path exercised indirectly: build() on a temp crate.
        let root = std::env::temp_dir().join(format!("harna-sigfix-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/m.rs"),
            "pub fn real() {}\n\n#[cfg(test)]\nmod tests {\n    \
pub fn phantom() {}\n}",
        )
        .unwrap();
        let d = build(&root, 100).unwrap();
        let m = d.modules.iter().find(|m| m.path == "src/m.rs").unwrap();
        assert_eq!(m.signatures, vec!["pub fn real() {}".to_string()]);
    }

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
