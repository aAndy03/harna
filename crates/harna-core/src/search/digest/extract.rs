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
    // Local names shadow crate names (0.3.46, ruff: `use crate::rules::ruff;`
    // then `ruff::rules::…` is the module, not the `ruff` crate — reading
    // it as the crate invented cross-crate cycles Cargo makes impossible).
    // A name a `use` brings into scope or a declared module is local.
    let mut names = name_index(crate_names);
    for local in local_names(src).iter().chain(sibling_mods) {
        if local != "crate" {
            names.remove(local);
        }
    }
    // A `use` statement may span lines (`use std::{\n path::PathBuf,\n};`):
    // it is gathered whole, so a continuation line is never read as an
    // import of a workspace crate that shares a std module's name (zed has
    // crates named `path`, `fs`, `channel`) (0.3.46).
    let mut pending: Option<String> = None;
    let mut strings = super::live::StrState::default();
    for (_, trimmed) in super::live::live_code(src) {
        // Paths inside string literals are text (0.3.47): read code only.
        let code = super::live::code_only(trimmed, &mut strings);
        let trimmed = code.as_str();
        // 1) use statements
        let starts_use = ["use ", "pub use ", "pub(crate) use "]
            .iter()
            .find_map(|p| trimmed.strip_prefix(p));
        if pending.is_some() || starts_use.is_some() {
            let piece = starts_use.unwrap_or(trimmed);
            let acc = pending.get_or_insert_with(String::new);
            acc.push_str(piece);
            acc.push(' ');
            if !trimmed.contains(';') {
                continue;
            }
            let stmt = pending.take().unwrap_or_default();
            for path in use_paths(stmt.trim().trim_end_matches(';')) {
                // `::krate::x` is the extern crate spelled absolutely (0.9.2)
                let path = path.trim_start_matches("::").to_string();
                let first = path.split("::").next().unwrap_or("");
                let rooted = matches!(first, "crate" | "super" | "self")
                    || crate_names.iter().any(|n| crate_name_eq(n, first));
                // Rust 2018 uniform paths: `use amount::Money;` names the
                // declared child module `amount` (0.9.2: a crate root's
                // `pub use amount::Money` was no edge, so nothing followed
                // the re-export to the module defining it)
                let path = if !rooted && sibling_mods.iter().any(|m| m == first) {
                    format!("self::{path}")
                } else if rooted {
                    path
                } else {
                    continue;
                };
                if !out.contains(&path) {
                    out.push(path);
                }
            }
            continue;
        }

        // 1b) a `use` later in the line: inside a body (`{ use api::x; … }`)
        // or after another statement (0.9.2, field test)
        for stmt in trimmed
            .split(['{', ';'])
            .skip(1)
            .filter_map(|piece| piece.trim_start().strip_prefix("use "))
        {
            for path in use_paths(stmt.trim()) {
                let path = path.trim_start_matches("::").to_string();
                let first = path.split("::").next().unwrap_or("");
                let rooted = matches!(first, "crate" | "super" | "self")
                    || crate_names.iter().any(|n| crate_name_eq(n, first));
                if rooted && !out.contains(&path) {
                    out.push(path);
                }
            }
        }

        // 2) inline path references
        for dep in inline_refs(trimmed, &names) {
            if !out.contains(&dep) {
                out.push(dep);
            }
        }

        // 3) bare sibling-module paths in expression position: a file may
        // reach its own declared submodules without any rooted prefix
        // (`rule_bindings::Bindings` from validate's mod.rs) — legal Rust,
        // and exactly what only-rooted extraction missed, making such
        // importers invisible to parity fan-out, dependents and whereis.
        // `use` lines are read above (a sibling path there reads as
        // `self::`, 0.9.2).
        {
            for dep in inline_sibling_refs(trimmed, sibling_mods) {
                if !out.contains(&dep) {
                    out.push(dep);
                }
            }
        }
    }
    out
}

/// The names a file brings into scope with `use` (the last segment, or
/// the alias after `as`), whole statements gathered across lines.
pub fn local_names(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending: Option<String> = None;
    for (_, line) in super::live::live_code(src) {
        let starts = ["use ", "pub use ", "pub(crate) use "]
            .iter()
            .find_map(|p| line.strip_prefix(p));
        if pending.is_none() && starts.is_none() {
            continue;
        }
        let acc = pending.get_or_insert_with(String::new);
        acc.push_str(starts.unwrap_or(line));
        acc.push(' ');
        if !line.contains(';') {
            continue;
        }
        let stmt = pending.take().unwrap_or_default();
        let stmt = stmt.trim().trim_end_matches(';');
        // aliases first: `x as y` makes `y` local
        for part in stmt.split([',', '{', '}']) {
            if let Some((_, alias)) = part.split_once(" as ") {
                let a = alias.trim();
                if !a.is_empty() && a != "_" {
                    out.push(a.to_string());
                }
            }
        }
        for path in use_paths(stmt) {
            if let Some(last) = path.rsplit("::").next() {
                if !last.is_empty() && last != "self" && last != "*" {
                    out.push(last.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The paths one `use` statement imports: `a::b::{C, d::E, self}` ->
/// `a::b::C`, `a::b::d::E`, `a::b`; nested groups expand one more level
/// by their head; `as` aliases and globs drop to their path.
pub fn use_paths(stmt: &str) -> Vec<String> {
    let stmt: String = stmt.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some(open) = stmt.find('{') else {
        let path = stmt.split(" as ").next().unwrap_or(&stmt).trim();
        let path = path.trim_end_matches("::*").trim_end_matches("::");
        return if path.is_empty() {
            Vec::new()
        } else {
            vec![path.replace(' ', "")]
        };
    };
    let prefix = stmt[..open].trim().trim_end_matches("::").replace(' ', "");
    let body = stmt[open + 1..].trim_end().trim_end_matches('}');
    // split the group's top level on commas
    let (mut items, mut depth, mut cur) = (Vec::new(), 0i32, String::new());
    for c in body.chars() {
        match c {
            '{' => {
                depth += 1;
                cur.push(c);
            }
            '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => items.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    items.push(cur);
    let mut out = Vec::new();
    for item in items.iter().map(|i| i.trim()).filter(|i| !i.is_empty()) {
        let head = item.split('{').next().unwrap_or(item);
        let head = head.split(" as ").next().unwrap_or(head).trim();
        let head = head
            .trim_end_matches("::*")
            .trim_end_matches("::")
            .replace(' ', "");
        let path = match head.as_str() {
            "" | "self" | "*" => prefix.clone(),
            h => format!("{prefix}::{h}"),
        };
        if !path.is_empty() && !out.contains(&path) {
            out.push(path);
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
#[cfg(test)]
pub(super) fn inline_crate_refs(line: &str, crate_names: &[String]) -> Vec<String> {
    inline_refs(line, &name_index(crate_names))
}

/// Import spelling -> canonical crate name, plus `crate` itself. Built
/// once per file (0.3.46: rebuilding ~800 needles per line made zed's
/// 1.78M-line scan take 15s).
pub(super) fn name_index(crate_names: &[String]) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    map.insert("crate".to_string(), "crate".to_string());
    for name in crate_names {
        map.insert(name.clone(), name.clone());
        map.insert(name.replace('-', "_"), name.clone());
    }
    map
}

/// Every `<known>::a::b` path in the line: one pass over its `::`
/// separators, the identifier before each looked up in `names`. A word
/// boundary is required — `my_crate::x` is not `crate::x`.
pub(super) fn inline_refs(
    line: &str,
    names: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(pos) = line[from..].find("::") {
        let sep = from + pos;
        from = sep + 2;
        // the identifier immediately before the separator
        let head_start = line[..sep]
            .char_indices()
            .rev()
            .take_while(|(_, c)| ident(*c))
            .last()
            .map(|(i, _)| i)
            .unwrap_or(sep);
        if head_start == sep {
            continue;
        }
        // not the tail of a longer path (`a::crate::x`) — but a leading
        // `::` (`::api::run()`, the extern crate spelled absolutely) is
        // the path's start (0.9.2)
        if head_start >= 1 && bytes[head_start - 1] == b':' {
            let absolute = head_start >= 2
                && bytes[head_start - 2] == b':'
                && line[..head_start - 2]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !(ident(c) || c == '>' || c == ':'));
            if !absolute {
                continue;
            }
        }
        let Some(name) = names.get(&line[head_start..sep]) else {
            continue;
        };
        let coll = &line[sep + 2..];
        let end = coll
            .find(|c: char| !(ident(c) || c == ':'))
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
        from = sep + 2 + end;
        if from >= line.len() {
            break;
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
        for (idx, (_, line)) in live.iter().enumerate() {
            let (line, trimmed) = (*line, *line);
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
                // a signature wrapped over lines (rustfmt) is read whole, up to
                // its body or `;` (0.9.2, codex field test: a parameter added
                // on a continuation line was "surface stable")
                let mut sig = trimmed.trim_end().to_string();
                if pub_rest.starts_with("fn ") && !sig.contains('{') && !sig.contains(';') {
                    for (_, next) in live.iter().skip(idx + 1).take(30) {
                        sig.push(' ');
                        sig.push_str(next.trim());
                        if next.contains('{') || next.contains(';') {
                            break;
                        }
                    }
                    sig = sig
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .replace("( ", "(")
                        .replace(", )", ")")
                        .replace(",)", ")");
                }
                signatures.push(sig);
            }
        }
        if !whole_test {
            signatures.extend(trait_methods(&super::live::live_code(&src)));
        }
        for name in super::tree::decls::file_mods(&src) {
            if !mods.contains(&name) {
                mods.push(name);
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

/// The methods of every public trait, as surface (0.3.49, ledger field
/// test): `fn insert(&self, a: &Account) -> Result<()>;` inside `pub trait
/// Repo` is API its implementors and callers depend on, yet carries no
/// `pub`. Each is recorded whole — a rustfmt-wrapped parameter list
/// joined onto one line — as `trait Repo: fn insert(…) -> Result<()>`, so a
/// changed parameter is a removed + added signature for parity.
fn trait_methods(live: &[(usize, &str)]) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut trait_name: Option<String> = None;
    let mut pending: Option<String> = None;
    for (_, line) in live {
        let t = line.trim();
        if let Some(sig) = pending.as_mut() {
            sig.push(' ');
            sig.push_str(t);
        } else if trait_name.is_some() && depth == 1 {
            let bare = ["async ", "unsafe ", "const "]
                .iter()
                .fold(t, |acc, q| acc.strip_prefix(q).unwrap_or(acc));
            if bare.starts_with("fn ") {
                pending = Some(t.to_string());
            }
        } else if trait_name.is_none() {
            let vis = t.strip_prefix("pub ").or_else(|| {
                t.starts_with("pub(")
                    .then(|| t.split_once(')').map(|(_, r)| r.trim_start()))
                    .flatten()
            });
            let rest = vis.map(|r| r.strip_prefix("unsafe ").unwrap_or(r));
            if let Some(rest) = rest.and_then(|r| r.strip_prefix("trait ")) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                trait_name = Some(name);
                depth = 0;
            }
        }
        if let Some(sig) = &pending {
            if sig.ends_with(';') || sig.ends_with('{') || sig.contains("{ ") {
                let body_start = sig.find(" {").or_else(|| sig.find('{'));
                let clean = match body_start {
                    Some(at) => sig[..at].trim_end(),
                    None => sig.trim_end_matches(';').trim_end(),
                };
                let clean = clean
                    .replace(", )", ")")
                    .replace(",)", ")")
                    .replace("( ", "(")
                    .replace(" )", ")");
                if let Some(name) = &trait_name {
                    out.push(format!("trait {name}: {clean}"));
                }
                pending = None;
            }
        }
        if trait_name.is_some() {
            depth += super::live::brace_delta(t);
            if depth <= 0 && t.contains('}') {
                trait_name = None;
                depth = 0;
            }
        }
    }
    out
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
