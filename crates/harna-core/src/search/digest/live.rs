//! `live` — the production lines of a Rust source: test-gated items
//! (`#[cfg(test)]` / `#[test]` / `mod tests`) and comment lines masked out.
//!
//! The mask is per item, not "everything after the first test marker": a
//! `#[cfg(test)] use …;` or `#[cfg(test)] mod fixtures;` at the top of a file
//! must not hide the code below it (0.3.42 — the font-manager probe found a
//! `std::fs::read` that the gateway check skipped exactly that way).
//!
//! Macro bodies that *emit* code are masked too (0.3.47, zed study H9):
//! `quote! { gpui::Bench… }` in a proc-macro crate is code the macro writes
//! into its callers, not a dependency of the macro crate. `quote!`,
//! `quote_spanned!`, `parse_quote!` and `macro_rules!` bodies are skipped;
//! the live code around them on the same line is kept.

/// (1-based line number, trimmed line) for every line outside test items
/// and comments.
pub fn live_code(src: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut state = State::Live;
    // Open emitted-code macro body: its delimiter depth (0 = none; -1 = the
    // marker was seen, its opening delimiter is still ahead).
    let mut macro_depth: i32 = 0;
    // Inside a block comment that started at the head of an earlier line
    // (`/* … */`, JSDoc `/** … */`; review of 1.1.2: TypeScript doc
    // comments were read as code).
    let mut in_block = false;
    for (i, line) in src.lines().enumerate() {
        let mut trimmed = line.trim();
        let masked = in_block || trimmed.starts_with("/*");
        if in_block {
            match trimmed.find("*/") {
                Some(end) => {
                    in_block = false;
                    trimmed = trimmed[end + 2..].trim();
                }
                None => continue,
            }
        }
        while trimmed.starts_with("/*") {
            match trimmed[2..].find("*/") {
                Some(end) => trimmed = trimmed[end + 4..].trim(),
                None => {
                    in_block = true;
                    trimmed = "";
                }
            }
        }
        if masked && trimmed.is_empty() {
            continue;
        }
        if macro_depth != 0 && matches!(state, State::Live) {
            let (pieces, depth) = emitted_split(trimmed, macro_depth);
            macro_depth = depth;
            for piece in pieces {
                if !piece.starts_with("//") {
                    out.push((i + 1, piece));
                }
            }
            continue;
        }
        match state {
            State::Inside(depth) => {
                let d = depth + brace_delta(trimmed);
                state = if d <= 0 {
                    State::Live
                } else {
                    State::Inside(d)
                };
                continue;
            }
            State::Pending => {
                state = open_item(trimmed);
                continue;
            }
            State::Live => {}
        }
        if let Some(rest) = test_marker(trimmed) {
            state = if rest.is_empty() {
                State::Pending
            } else {
                open_item(rest)
            };
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        if MACROS.iter().any(|m| trimmed.contains(m)) {
            let (pieces, depth) = emitted_split(trimmed, 0);
            macro_depth = depth;
            for piece in pieces {
                out.push((i + 1, piece));
            }
            continue;
        }
        out.push((i + 1, trimmed));
    }
    out
}

/// Macros whose bodies are code written into the caller, not code run here.
const MACROS: &[&str] = &["quote!", "quote_spanned!", "parse_quote!", "macro_rules!"];

/// The live pieces of a line given the emitted-code depth it starts at, and
/// the depth it leaves open (0 closed, -1 marker seen but not yet opened).
fn emitted_split(line: &str, mut depth: i32) -> (Vec<&str>, i32) {
    let mut pieces = Vec::new();
    let mut i = 0usize;
    loop {
        if depth != 0 {
            match close_from(line, i, depth) {
                Ok(end) => i = end,
                Err(d) => return (pieces, d),
            }
        }
        let rest = &line[i..];
        let Some((at, len)) = MACROS
            .iter()
            .filter_map(|m| marker_at(rest, m).map(|a| (a, m.len())))
            .min_by_key(|(a, _)| *a)
        else {
            let tail = rest.trim();
            if !tail.is_empty() && !tail.starts_with("//") {
                pieces.push(tail);
            }
            return (pieces, 0);
        };
        let before = rest[..at].trim();
        if !before.is_empty() {
            pieces.push(before);
        }
        i += at + len;
        depth = -1;
    }
}

/// A macro marker that starts a word (`my_quote!` is someone else's).
fn marker_at(s: &str, marker: &str) -> Option<usize> {
    s.match_indices(marker).map(|(a, _)| a).find(|&a| {
        s[..a]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
    })
}

/// Walk `line` from `from` at `depth` (-1: the opening delimiter is still
/// ahead) to where the body closes: Ok(position after it), or Err(depth
/// still open at the line's end). String and char literals are skipped.
fn close_from(line: &str, from: usize, mut depth: i32) -> Result<usize, i32> {
    let mut in_str = false;
    let mut iter = line[from..].char_indices().peekable();
    while let Some((k, c)) = iter.next() {
        if in_str {
            match c {
                '\\' => {
                    iter.next();
                }
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' | '[' | '{' => depth = if depth < 0 { 1 } else { depth + 1 },
            ')' | ']' | '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return Ok(from + k + c.len_utf8());
                }
            }
            _ => {}
        }
    }
    Err(depth)
}

/// Where a line starts relative to string literals: in code, or inside a
/// string opened on an earlier line (its raw `#` count; None: not raw).
#[derive(Clone, Copy, Default)]
pub(super) struct StrState(Option<Option<usize>>);

/// `line` with string-literal contents removed (the quotes stay): a path
/// written in a message (`"… gpui::bench_main! …"`) is text, not a
/// reference (0.3.47, zed). `state` carries an open string to the next line.
pub(super) fn code_only(line: &str, state: &mut StrState) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if let Some(raw) = state.0 {
            // inside a string: find its end
            match raw {
                None => match chars[i] {
                    '\\' => i += 2,
                    '"' => {
                        out.push('"');
                        state.0 = None;
                        i += 1;
                    }
                    _ => i += 1,
                },
                Some(n) => {
                    if chars[i] == '"'
                        && chars[i + 1..].iter().take(n).filter(|c| **c == '#').count() == n
                    {
                        out.push('"');
                        state.0 = None;
                        i += 1 + n;
                    } else {
                        i += 1;
                    }
                }
            }
            continue;
        }
        let c = chars[i];
        let prev_ident = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            break;
        }
        if c == '"' {
            out.push('"');
            state.0 = Some(None);
            i += 1;
        } else if (c == 'r' || (c == 'b' && chars.get(i + 1) == Some(&'r'))) && !prev_ident {
            // raw string: r"…", r#"…"#, br#"…"#
            let start = if c == 'b' { i + 2 } else { i + 1 };
            let hashes = chars[start..].iter().take_while(|c| **c == '#').count();
            if chars.get(start + hashes) == Some(&'"') {
                out.push('"');
                state.0 = Some(Some(hashes));
                i = start + hashes + 1;
            } else {
                out.push(c);
                i += 1;
            }
        } else if c == '\'' {
            // char literal ('"', '\"'); a lifetime has no closing quote
            if chars.get(i + 1) == Some(&'\\') {
                let close = chars[i + 2..].iter().position(|c| *c == '\'');
                i = close.map(|k| i + 3 + k).unwrap_or(i + 1);
            } else if chars.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                out.push(c);
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[derive(Clone, Copy)]
enum State {
    Live,
    /// a test attribute was seen; the item it gates has not started yet
    Pending,
    /// inside a gated item's braces, at this depth
    Inside(i32),
}

/// The text after a test marker on this line, if the line opens one.
/// `mod tests` (without an attribute) is itself the gated item.
fn test_marker(trimmed: &str) -> Option<&str> {
    for attr in ["#[cfg(test)]", "#[test]"] {
        if let Some(rest) = trimmed.strip_prefix(attr) {
            return Some(rest.trim());
        }
    }
    // `#[cfg(all(test, target_os = "linux"))]` gates on test too (0.3.47,
    // zed's sandbox): an `all(…)` with a bare `test` among its arguments.
    if let Some(pred) = trimmed.strip_prefix("#[cfg(all(") {
        let end = pred.find(")]")?;
        let (args, rest) = (&pred[..end], &pred[end + 2..]);
        let args = args.strip_suffix(')').unwrap_or(args);
        if args.split(',').any(|a| a.trim() == "test") {
            return Some(rest.trim());
        }
    }
    if trimmed.starts_with("mod tests") || trimmed.starts_with("pub mod tests") {
        return Some(trimmed);
    }
    None
}

/// Where the gated item stands after its first (or a continuation) line.
fn open_item(line: &str) -> State {
    if line.is_empty() || line.starts_with("#[") || line.starts_with("//") {
        return State::Pending; // more attributes/comments before the item
    }
    if line.contains('{') {
        let d = brace_delta(line);
        return if d <= 0 {
            State::Live
        } else {
            State::Inside(d)
        };
    }
    if line.ends_with(';') {
        State::Live // `#[cfg(test)] mod fixtures;` / `use …;`
    } else {
        State::Pending // multi-line signature: the brace is still ahead
    }
}

/// `{` minus `}` outside string/char literals and trailing line comments.
pub fn brace_delta(line: &str) -> i32 {
    let mut d = 0;
    let mut in_str = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '\'' => {
                // char literal `'{'` / `'\''`; a lifetime `'a` has no closing quote
                let ahead: String = chars.clone().take(3).collect();
                if let Some(escaped) = ahead.strip_prefix('\\') {
                    if let Some(end) = escaped.find('\'') {
                        for _ in 0..end + 2 {
                            chars.next();
                        }
                    }
                } else if ahead.chars().nth(1) == Some('\'') {
                    chars.next();
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'/') => break,
            '{' => d += 1,
            '}' => d -= 1,
            _ => {}
        }
    }
    d
}

#[cfg(test)]
mod tests;
