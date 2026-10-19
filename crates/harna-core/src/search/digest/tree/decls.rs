//! `decls` (0.3.47, zed/ruff/codex/deno study H10): the files a Rust source
//! declares — what the module tree (`tree`) follows to tell source from
//! data. Lexical, line by line, like the rest of the extractor: `mod`
//! declarations (test-gated, nested in inline modules, raw identifiers),
//! `#[path]` / `#[cfg_attr(…, path)]` attributes (multi-line too) and
//! `include!("…")`.

/// Every file a source declares with `mod NAME;` — test-gated ones too
/// (`#[cfg(test)] mod tests;` is a file the module tree reaches even though
/// live code masks it), nested ones as their path: `mod m { mod k; }` is
/// `m/k` (0.3.47, zed's migrator), `mod r#enum;` as `enum` (deno), and a
/// file named by `#[path = "…"]`, `#[cfg_attr(…, path = "…")]` (every
/// alternative — codex picks a backend per OS) or `include!("…")` as its
/// literal path (the `.rs` suffix marks it; relative to the declaring
/// file's directory). Feeds orphan detection.
pub fn file_mods(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    // Open inline modules: (name, depth inside their braces).
    let mut open: Vec<(String, i32)> = Vec::new();
    // `path = "…"` attributes waiting for the `mod` they decorate.
    let mut path_attrs: Vec<String> = Vec::new();
    // An attribute spread over lines (`#[cfg_attr(\n  …,\n  path = "…"\n)]`)
    // is gathered whole before it is read.
    let mut attr_buf = String::new();
    for line in src.lines() {
        let raw = line.trim();
        if raw.starts_with("//") || raw.is_empty() {
            continue;
        }
        if !attr_buf.is_empty() || (raw.starts_with("#[") && bracket_balance(raw) > 0) {
            attr_buf.push_str(raw);
            attr_buf.push(' ');
            if bracket_balance(&attr_buf) > 0 {
                continue;
            }
        }
        let gathered = std::mem::take(&mut attr_buf);
        let mut t = if gathered.is_empty() {
            raw
        } else {
            gathered.trim()
        };
        for p in included_files(t) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        let attributed = t.starts_with("#[");
        while t.starts_with("#[") {
            let Some(end) = t.find(']') else { break };
            let attr = &t[..end];
            if attr.starts_with("#[path") || attr.starts_with("#[cfg_attr") {
                if let Some(p) = attr.match_indices("path").find_map(|(at, _)| {
                    let r = attr[at + 4..].trim_start().strip_prefix('=')?.trim_start();
                    r.strip_prefix('"')?.split('"').next()
                }) {
                    path_attrs.push(p.to_string());
                }
            }
            t = t[end + 1..].trim_start();
        }
        if attributed && t.is_empty() {
            continue; // attribute-only line: the item is below
        }
        let paths = std::mem::take(&mut path_attrs);
        if t.starts_with("pub(") {
            t = t.split_once(')').map(|(_, r)| r.trim_start()).unwrap_or(t);
        }
        let t = t.strip_prefix("pub ").unwrap_or(t);
        if let Some(rest) = t.strip_prefix("mod ") {
            let rest = rest.strip_prefix("r#").unwrap_or(rest);
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let after = rest[name.len()..].trim_start();
            if !name.is_empty() {
                let mut path: Vec<&str> = open.iter().map(|(n, _)| n.as_str()).collect();
                path.push(&name);
                let path = path.join("/");
                if after.starts_with(';') {
                    if paths.is_empty() {
                        out.push(path);
                    } else if open.is_empty() {
                        // inside an inline module the base shifts: not followed
                        out.extend(paths.into_iter().filter(|p| p.ends_with(".rs")));
                    }
                } else if after.starts_with('{') {
                    open.push((name, depth + 1));
                }
            }
        }
        depth += super::super::live::brace_delta(line);
        while open.last().is_some_and(|(_, d)| *d > depth) {
            open.pop();
        }
    }
    out
}

/// `[` minus `]` outside string literals.
fn bracket_balance(s: &str) -> i32 {
    let mut in_str = false;
    let mut n = 0;
    for c in s.chars() {
        match c {
            '"' => in_str = !in_str,
            '[' if !in_str => n += 1,
            ']' if !in_str => n -= 1,
            _ => {}
        }
    }
    n
}

/// The `.rs` files a line splices in with `include!("…")` (a literal path;
/// `include!(concat!(env!("OUT_DIR"), …))` names a build output, not a file
/// in the tree).
fn included_files(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, _) in line.match_indices("include!(") {
        let word_start = line[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        let lit = line[at + "include!(".len()..]
            .trim_start()
            .strip_prefix('"')
            .and_then(|r| r.split('"').next());
        if let Some(p) = lit.filter(|p| word_start && p.ends_with(".rs")) {
            out.push(p.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests;
