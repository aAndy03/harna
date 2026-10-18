//! `live` — the production lines of a Rust source: test-gated items
//! (`#[cfg(test)]` / `#[test]` / `mod tests`) and comment lines masked out.
//!
//! The mask is per item, not "everything after the first test marker": a
//! `#[cfg(test)] use …;` or `#[cfg(test)] mod fixtures;` at the top of a file
//! must not hide the code below it (0.3.42 — the font-manager probe found a
//! `std::fs::read` that the gateway check skipped exactly that way).

/// (1-based line number, trimmed line) for every line outside test items
/// and comments.
pub fn live_code(src: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut state = State::Live;
    for (i, line) in src.lines().enumerate() {
        let trimmed = line.trim();
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
        out.push((i + 1, trimmed));
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
fn brace_delta(line: &str) -> i32 {
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
