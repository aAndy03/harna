//! Component `ownership` (0.3.45, deno study B1/B2/B5): what a container
//! `owns`, only it constructs or declares — and every escape hatch out of
//! an owned capability is counted.
//!
//! - A **capability type** (`owns = ["CheckedPath"]`): building it outside
//!   the owner is a violation — a struct literal (`CheckedPath {`) or a
//!   constructor call (`CheckedPath::new(`, `::from(`, `::try_from(`,
//!   `::default(`, `::new_*(`). Holding, passing and naming it are fine:
//!   deno's permission layer is exactly this (private fields, `FileSystem`
//!   methods take `&CheckedPath`); harna records the line and guards it
//!   even where the type system is bypassed.
//! - A **marker** (`owns = ["#[op2", "extension!"]`, anything starting
//!   `#[` or ending `!`): appears nowhere outside the owner (deno's doc
//!   says ops live in `ext/`; `cli/ops/` declares them too).
//! - An **escape hatch** (`escapes = ["CheckedPath::unsafe_new"]`): every
//!   call, anywhere, is one `escape-hatch` error — baseline them and the
//!   count can only fall (deno: 42 `unsafe_new` sites in 5 files).
//!
//! Test items and comments are masked (`search::digest::live`).

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::model::manifest::Manifest;
use crate::schema::model::scope::path_within;
use crate::search::digest::live::live_code;

const CONSTRUCTORS: &[&str] = &["new(", "from(", "try_from(", "default(", "new_"];

pub fn check(manifest: &Manifest, srcs: &[(String, String)], out: &mut Vec<Violation>) {
    for c in manifest.layers.iter().flat_map(|l| &l.containers) {
        if c.path.is_empty() || (c.owns.is_empty() && c.escapes.is_empty()) {
            continue;
        }
        for (file, src) in srcs {
            let inside = path_within(file, &c.path);
            for (n, line) in live_code(src) {
                if !inside {
                    for tok in &c.owns {
                        if let Some(how) = violates(line, tok) {
                            out.push(Violation {
                                check: "ownership".into(),
                                file: file.clone(),
                                line: n,
                                severity: severity("ownership"),
                                message: format!(
                                    "{how} `{tok}`, which container `{}` owns",
                                    c.name
                                ),
                                hint: format!(
                                    "go through `{}` (its constructor or registration), or move this code into `{}`",
                                    c.name, c.path
                                ),
                            });
                        }
                    }
                }
                for hatch in &c.escapes {
                    if calls(line, hatch) {
                        out.push(Violation {
                            check: "escape-hatch".into(),
                            file: file.clone(),
                            line: n,
                            severity: severity("escape-hatch"),
                            message: format!(
                                "escape hatch `{hatch}` out of `{}`'s capability",
                                c.name
                            ),
                            hint: "prefer the checked path; existing sites: `harna baseline record --check escape-hatch` so the count only falls".into(),
                        });
                    }
                }
            }
        }
    }
}

fn severity(id: &str) -> Severity {
    checks::spec(id)
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Error)
}

fn is_marker(tok: &str) -> bool {
    tok.starts_with("#[") || tok.ends_with('!')
}

/// How `line` breaks ownership of `tok`, if it does.
fn violates(line: &str, tok: &str) -> Option<&'static str> {
    if is_marker(tok) {
        return line.contains(tok).then_some("declares the marker");
    }
    let mut from = 0;
    while let Some(at) = line[from..].find(tok) {
        let at = from + at;
        from = at + tok.len();
        let before = &line[..at];
        let after = &line[at + tok.len()..];
        let word_start = before
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if !word_start {
            continue;
        }
        if let Some(rest) = after.strip_prefix("::") {
            if CONSTRUCTORS.iter().any(|c| rest.starts_with(c)) {
                return Some("constructs");
            }
            continue;
        }
        let declares = [
            "struct ", "enum ", "union ", "impl ", "for ", "trait ", "type ", "<",
        ]
        .iter()
        .any(|kw| before.trim_end().ends_with(kw.trim_end()));
        let next = after.trim_start();
        let continues_word = after
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if next.starts_with('{')
            && !declares
            && !continues_word
            && !before.trim_end().ends_with("->")
        {
            return Some("constructs");
        }
    }
    None
}

/// A call (or path use) of the escape hatch, not its definition.
fn calls(line: &str, hatch: &str) -> bool {
    let name = hatch.rsplit("::").next().unwrap_or(hatch);
    if line.contains(&format!("fn {name}")) {
        return false;
    }
    line.contains(hatch)
}

#[cfg(test)]
mod tests;
