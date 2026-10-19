//! `groups` (0.3.47): grouped imports a move must see.
//!
//! `use crate::commands::{registers::ack, other};` never spells
//! `commands::registers::ack` as one token run, so a path rewrite misses
//! it. A grouped statement that imports a moved path is flattened — one
//! `use` per leaf, aliases, globs and `self` kept — and its leaves are
//! rewritten like any other path; untouched statements keep their shape.

/// Every leaf a `use` tree imports, losslessly: `a::{b::{C as D, *}, self}`
/// -> `a::b::C as D`, `a::b::*`, `a`.
pub fn flatten(tree: &str) -> Vec<String> {
    let tree: String = tree.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    leaves("", tree.trim(), &mut out);
    out
}

fn leaves(prefix: &str, tree: &str, out: &mut Vec<String>) {
    let join = |p: &str, t: &str| {
        if p.is_empty() {
            t.to_string()
        } else {
            format!("{p}::{t}")
        }
    };
    let Some(open) = tree.find('{') else {
        let t = tree.trim();
        match t {
            "" => {}
            "self" => out.push(prefix.to_string()),
            t if t.starts_with("self as ") => out.push(format!("{prefix} {}", &t[5..])),
            t => out.push(join(prefix, t)),
        }
        return;
    };
    let head = tree[..open].trim().trim_end_matches("::").replace(' ', "");
    let base = join(prefix, &head);
    let body = tree[open + 1..].trim_end();
    let body = body.strip_suffix('}').unwrap_or(body);
    let (mut depth, mut cur) = (0i32, String::new());
    for c in body.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                leaves(&base, &std::mem::take(&mut cur), out);
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    leaves(&base, &cur, out);
}

/// Flatten each grouped `use` statement in `src` one of whose leaves
/// `touches` (a moved path); `rewrite` then maps every leaf. Returns the
/// new source when a statement changed.
pub fn expand(
    src: &str,
    touches: impl Fn(&str) -> bool,
    rewrite: impl Fn(&str) -> String,
) -> Option<String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut changed = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let indent = &line[..line.len() - line.trim_start().len()];
        let t = line.trim_start();
        let (vis, rest) = split_vis(t);
        if !rest.starts_with("use ") {
            out.push(line.to_string());
            i += 1;
            continue;
        }
        // gather the statement to its `;`
        let mut stmt = rest["use ".len()..].to_string();
        let mut end = i;
        while !stmt.contains(';') && end + 1 < lines.len() {
            end += 1;
            stmt.push(' ');
            stmt.push_str(lines[end].trim());
        }
        let Some(tree) = stmt.split(';').next().filter(|_| stmt.contains('{')) else {
            out.extend(lines[i..=end].iter().map(|l| l.to_string()));
            i = end + 1;
            continue;
        };
        let leaves = flatten(tree);
        if !leaves.iter().any(|l| touches(l)) {
            out.extend(lines[i..=end].iter().map(|l| l.to_string()));
            i = end + 1;
            continue;
        }
        for leaf in leaves {
            out.push(format!("{indent}{vis}use {};", rewrite(&leaf)));
        }
        changed = true;
        i = end + 1;
    }
    if !changed {
        return None;
    }
    let mut text = out.join("\n");
    if src.ends_with('\n') {
        text.push('\n');
    }
    Some(text)
}

/// A rewritten leaf keeps the name the file used: `a::old_name` moved to
/// `b::new_name` imports `b::new_name as old_name`, so the file's uses of
/// `old_name` still resolve. An aliased or unchanged leaf stays as is.
pub fn keep_name(leaf: &str, rewritten: Option<String>) -> String {
    let Some(new) = rewritten else {
        return leaf.to_string();
    };
    let last = |p: &str| p.rsplit("::").next().unwrap_or(p).to_string();
    if leaf.contains(" as ") || leaf.ends_with('*') || last(leaf) == last(&new) {
        return new;
    }
    format!("{new} as {}", last(leaf))
}

/// `pub(crate) use …` -> (`pub(crate) `, `use …`).
fn split_vis(t: &str) -> (&str, &str) {
    if t.starts_with("pub(") {
        if let Some(close) = t.find(')') {
            let rest = t[close + 1..].trim_start();
            return (&t[..t.len() - rest.len()], rest);
        }
    }
    if let Some(rest) = t.strip_prefix("pub ") {
        return ("pub ", rest);
    }
    ("", t)
}

#[cfg(test)]
mod tests;
