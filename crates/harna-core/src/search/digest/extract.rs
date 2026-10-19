//! Extraction: one Rust source file into a map module — internal deps,
//! declared names and mods, the body hash that lets scans reuse work.

use super::*;

/// Extract a module's internal dependency list.
///
/// Two sources, both test-excluded:/// - `use` statements (`crate::a::b::{X, Y}` collapses to `crate::a::b`)
/// - inline path references in expressions/calls, e.g. `crate::a::b::f()`
///   or `harna_core::schema::store::Store` — so an importer that never
///   declares a `use` still receives propagation fan-out.
///
/// Dep ids keep the raw path shape; only `crate::`/`super::`/`self::` and
/// workspace-crate imports count (std and external crates excluded).
pub fn extract_deps(src: &str, crate_names: &[String], sibling_mods: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, trimmed) in super::live::live_code(src) {
        // 1) use statements
        if let Some(rest) = trimmed.trim_end_matches(';').strip_prefix("use ") {
            let head = rest
                .split('{')
                .next()
                .unwrap_or(rest)
                .trim()
                .trim_end_matches("::")
                .to_string();
            if !head.is_empty() && head.contains("::") {
                let first_segment = head.split("::").next().unwrap_or("");
                let rooted = head.starts_with("crate::")
                    || head.starts_with("super::")
                    || head.starts_with("self::")
                    || crate_names.iter().any(|n| crate_name_eq(n, first_segment));
                if rooted && !out.contains(&head) {
                    out.push(head);
                }
            }
        }

        // 2) inline path references
        for dep in inline_crate_refs(trimmed, crate_names) {
            if !out.contains(&dep) {
                out.push(dep);
            }
        }

        // 3) bare sibling-module paths in expression position: a file may
        // reach its own declared submodules without any rooted prefix
        // (`rule_bindings::Bindings` from validate's mod.rs) — legal Rust,
        // and exactly what only-rooted extraction missed, making such
        // importers invisible to parity fan-out, dependents and whereis.
        // `use` lines cannot be bare sibling paths in Rust 2018+, so they
        // are exempt from this rule.
        if !trimmed.starts_with("use ") {
            for dep in inline_sibling_refs(trimmed, sibling_mods) {
                if !out.contains(&dep) {
                    out.push(dep);
                }
            }
        }
    }
    out
}

/// Collect bare sibling references (`sibling::item`) as `self::sibling::item`
/// dependency ids — resolvable against the importer's own module stem.
pub(super) fn inline_sibling_refs(line: &str, sibling_mods: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for m in sibling_mods {
        let needle = format!("{m}::");
        let mut from = 0usize;
        while let Some(pos) = line[from..].find(needle.as_str()) {
            let start = from + pos + needle.len();
            let coll = &line[start.min(line.len())..];
            let end = coll
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(coll.len());
            let segments: Vec<&str> = std::iter::once(m.as_str())
                .chain(
                    coll[..end]
                        .split("::")
                        .filter(|s| !s.is_empty() && *s != "*"),
                )
                .collect();
            if segments.len() > 1 {
                let dep = format!("self::{}", segments.join("::"));
                if !out.contains(&dep) {
                    out.push(dep);
                }
            }
            from = start + end;
            if from >= line.len() {
                break;
            }
        }
    }
    out
}

/// Collect internal path references appearing anywhere in a line
/// (`crate::a::b::c()`, `my_crate::x.y()`), as dependency ids.
pub(super) fn inline_crate_refs(line: &str, crate_names: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut needles: Vec<(String, String)> = vec![("crate".to_string(), "crate::".to_string())];
    for name in crate_names {
        needles.push((name.clone(), format!("{name}::")));
        // import sites normalize hyphens to underscores; cover both spellings
        let underscored = name.replace('-', "_");
        if underscored != *name {
            needles.push((name.clone(), format!("{underscored}::")));
        }
    }
    for (name, needle) in &needles {
        let mut from = 0usize;
        while let Some(pos) = line[from..].find(needle.as_str()) {
            let start = from + pos + needle.len();
            let coll = &line[start.min(line.len())..];
            let end = coll
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(coll.len());
            let segments: Vec<&str> = coll[..end]
                .split("::")
                .filter(|s| !s.is_empty() && *s != "*")
                .collect();
            if !segments.is_empty() {
                let dep = format!("{name}::{}", segments.join("::"));
                if !out.contains(&dep) {
                    out.push(dep);
                }
            }
            from = start + end;
            if from >= line.len() {
                break;
            }
        }
    }
    out
}

pub(super) fn scan_rust_file(root: &Path, path: &Path, crate_names: &[String]) -> Module {
    let rel = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let mut signatures = Vec::new();
    let mut inventory: Vec<Item> = Vec::new();
    let mut mods: Vec<String> = Vec::new();
    let mut deps: Vec<String> = Vec::new();
    let mut body_hash = String::new();
    let mut reexports: Vec<String> = Vec::new();
    if let Ok(src) = fs::read_to_string(path) {
        body_hash = hash_body(src.as_bytes());
        // Test sections are fixtures, not shipped surface: their items must
        // not appear as signatures (same rule as validate and deps). A whole
        // test file (0.3.37) is one test section from its first line.
        let whole_test = is_test_path(&rel);
        let live = if whole_test {
            Vec::new()
        } else {
            super::live::live_code(&src)
        };
        for (_, line) in &live {
            for name in reexported_names(line) {
                if !reexports.contains(&name) {
                    reexports.push(name);
                }
            }
        }
        for (_, line) in live {
            let trimmed = line;
            if let Some(item) = Item::parse(trimmed) {
                if item.kind == "mod" {
                    mods.push(item.name.clone());
                }
                inventory.push(item);
            }
            let pub_rest = line.trim_start().strip_prefix("pub ").unwrap_or("");
            // a visibility lid like pub(crate) ends at its own ')' — the
            // parameter list after the name closes with its own ')'.
            let pub_rest = if line.trim_start().starts_with("pub(") {
                line.trim_start()
                    .split_once(')')
                    .map(|(_, r)| r.trim_start())
                    .unwrap_or("")
            } else {
                pub_rest
            };
            if pub_rest.starts_with("fn ")
                || pub_rest.starts_with("struct ")
                || pub_rest.starts_with("enum ")
                || pub_rest.starts_with("trait ")
            {
                signatures.push(trimmed.trim_end().to_string());
            }
        }
        mods.sort();
        if !whole_test {
            deps = extract_deps(&src, crate_names, &mods);
        }
    }
    Module {
        reexports,
        crate_dir: None,
        src_root: None,
        path: rel,
        lang: "rust".into(),
        signatures,
        deps,
        body_hash,
        inventory,
        mods,
    }
}

/// Is this whole file test code? A module split into `tests.rs` (or
/// `*_test.rs` / `*_tests.rs`) via `#[cfg(test)] mod …;`, or anything
/// under a crate's `tests/` directory. Shared by the map and validate so
/// a test moved out of line keeps the exemptions it had inline.
pub fn is_test_path(rel: &str) -> bool {
    let rel = rel.replace('\\', "/");
    let name = rel.rsplit('/').next().unwrap_or(&rel);
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || name.ends_with("_test.rs")
        || rel.split('/').any(|seg| seg == "tests")
}

/// Content hash of a module body; lets `harna diff` observe body-only
/// changes and validate's body-hash reuse (0.3.17) prove a live file is
/// the body the map cataloged — one hash discipline, both consumers.
pub fn hash_body(bytes: &[u8]) -> String {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The declared names of a live file when the map can prove them current
/// (0.3.17 body-hash reuse): the inventory catalogs exactly what
/// `declared_names` derives (same filtered `Item::parse` loop — test code
/// and comments excluded), so a matching body hash means the catalog is
/// the parse, without re-parsing. A changed body, an empty hash, or an
/// older extractor returns None — the caller parses as before.
pub fn reusable_declared_names(module: &Module, src: &str) -> Option<Vec<String>> {
    if module.body_hash.is_empty() || hash_body(src.as_bytes()) != module.body_hash {
        return None;
    }
    let mut names: Vec<String> = module.inventory.iter().map(|i| i.name.clone()).collect();
    names.sort();
    names.dedup();
    Some(names)
}

/// Submodule names declared in a module body (comment lines and test
/// sections excluded) — the same list the scanner records as `mods`, exposed
/// so the boundary check can reproduce the sibling edges the map carries.
pub fn declared_mods(src: &str) -> Vec<String> {
    let mut mods = Vec::new();
    for (_, trimmed) in super::live::live_code(src) {
        if let Some(item) = Item::parse(trimmed) {
            if item.kind == "mod" {
                mods.push(item.name);
            }
        }
    }
    mods.sort();
    mods.dedup();
    mods
}

/// Declared item names (pub and private, comments and test sections
/// excluded) — the same names the inventory catalogs, exposed so the
/// pattern rules police exactly the names the map knows.
pub fn declared_names(src: &str) -> Vec<String> {
    let mut names = Vec::new();
    for (_, trimmed) in super::live::live_code(src) {
        if let Some(item) = Item::parse(trimmed) {
            names.push(item.name);
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Names a `pub use` line makes public: the last segment or its alias;
/// brace groups expand one level; a glob is `*`.
pub fn reexported_names(line: &str) -> Vec<String> {
    let Some(rest) = line
        .strip_prefix("pub use ")
        .or_else(|| line.strip_prefix("pub(crate) use ").map(|_| ""))
    else {
        return Vec::new();
    };
    let body = rest.trim().trim_end_matches(';').trim();
    let (head, group) = match body.split_once('{') {
        Some((h, g)) => (h, g.trim_end_matches('}')),
        None => ("", body),
    };
    let _ = head;
    group
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            if p.ends_with('*') {
                return Some("*".to_string());
            }
            let name = match p.split_once(" as ") {
                Some((_, alias)) => alias.trim(),
                None => p.rsplit("::").next().unwrap_or(p).trim(),
            };
            (!name.is_empty() && name != "self").then(|| name.to_string())
        })
        .collect()
}
