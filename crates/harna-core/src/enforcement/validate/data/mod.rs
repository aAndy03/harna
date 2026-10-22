//! Check `data-literal` (1.1.6): lists of facts live in the data home, not
//! in code.
//!
//! Prices, plans, country lists, menu entries, feature flags: an agent
//! writes them into the component that first needs them, and the next one
//! copies them. The bound rule names the data home in `values` (globs:
//! `data/**`, `web/src/data/*.json`); a literal list of `limit` or more
//! items (default 6) outside it is the finding. An item is a literal (a
//! string, a number, true/false/null) or a record of literals (`{ name:
//! "Pro", price: 29 }`, `("pro", 29)`).
//!
//! Text, not a parser: brackets are matched over comment-free code with
//! strings skipped.

use super::Violation;
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::{glob_match, path_within};

/// The least items a list needs to be data, when the rule sets no limit.
const DEFAULT_LIMIT: usize = 6;

pub fn check(rules: &Rules, env: Option<&str>, file: &str, src: &str, out: &mut Vec<Violation>) {
    for rule in rules.rules.iter().filter(|r| {
        !r.is_deprecated()
            && r.matches_env(env)
            && r.check.as_deref() == Some("data-literal")
            && (r.scope.is_empty() || r.scope.iter().any(|s| path_within(file, s)))
    }) {
        if rule.values.iter().any(|g| glob_match(g, file)) {
            continue; // the data home itself
        }
        let limit = rule.limit.map_or(DEFAULT_LIMIT, |l| l as usize);
        let rust = file.ends_with(".rs");
        let (text, lines) = live_text(src);
        for (at, items) in lists(&text, rust) {
            if items < limit {
                continue;
            }
            let line = lines
                .iter()
                .take_while(|(off, _)| *off <= at)
                .last()
                .map_or(0, |(_, n)| *n);
            out.push(Violation {
                check: "data-literal".into(),
                file: file.into(),
                line,
                severity: crate::enforcement::checks::severity_of("data-literal"),
                message: format!(
                    "a list of {items} facts written into code (rule `{}`): the next screen that needs them copies them",
                    rule.id
                ),
                hint: if rule.values.is_empty() {
                    "move the list to a data file the code reads (and name the data home in the rule's values)".into()
                } else {
                    format!("move the list into the data home ({}) and read it from there", rule.values.join(", "))
                },
            });
        }
    }
}

/// The comment-free code as one text, with each line's starting offset
/// and number.
fn live_text(src: &str) -> (String, Vec<(usize, usize)>) {
    let mut text = String::new();
    let mut lines = Vec::new();
    for (n, l) in crate::search::digest::live::live_code(src) {
        lines.push((text.len(), n));
        text.push_str(l);
        text.push('\n');
    }
    (text, lines)
}

/// Skip a string starting at `i` (its quote); returns the index after it.
fn skip_string(b: &[u8], i: usize) -> usize {
    let q = b[i];
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            c if c == q => return j + 1,
            _ => j += 1,
        }
    }
    j
}

/// Every `[ … ]` whose top-level items are all literals or records of
/// literals: (offset of the `[`, item count).
pub fn lists(text: &str, rust: bool) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' | b'`' => i = skip_string(b, i),
            b'\'' if !rust => i = skip_string(b, i),
            b'[' => {
                if let Some((end, items)) = literal_list(b, i, rust) {
                    if items > 0 {
                        out.push((i, items));
                    }
                    i = end;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// From an opening `[` at `start`: Some((index after the `]`, items)) when
/// every top-level item is a literal or a record of literals.
fn literal_list(b: &[u8], start: usize, rust: bool) -> Option<(usize, usize)> {
    let mut items = 0;
    let mut i = start + 1;
    let mut item = Vec::new();
    loop {
        let (end, kind) = one_item(b, i, rust)?;
        let raw = std::str::from_utf8(&b[i..end]).ok()?.trim().to_string();
        item.push(kind);
        if !raw.is_empty() {
            items += 1;
        }
        i = end;
        match b.get(i)? {
            b',' => i += 1,
            b']' => return item.iter().all(|k| *k).then_some((i + 1, items)),
            _ => return None,
        }
    }
}

/// One item from `i` up to the next top-level `,` or `]`: (its end,
/// whether it is literal data). Nested brackets and braces are walked.
fn one_item(b: &[u8], mut i: usize, rust: bool) -> Option<(usize, bool)> {
    let start = i;
    let mut depth = 0i32;

    while i < b.len() {
        let c = b[i];
        match c {
            b'"' | b'`' => {
                i = skip_string(b, i);
                continue;
            }
            b'\'' if !rust => {
                i = skip_string(b, i);
                continue;
            }
            b'[' | b'{' | b'(' => depth += 1,
            b']' | b'}' | b')' if depth > 0 => depth -= 1,
            b',' | b']' if depth == 0 => break,
            _ => {}
        }
        i += 1;
    }
    let raw = std::str::from_utf8(&b[start..i.min(b.len())]).ok()?;
    // what is left once strings are gone: keys, numbers, punctuation
    let mut bare = String::new();
    let rb = raw.as_bytes();
    let mut k = 0;

    while k < rb.len() {
        match rb[k] {
            b'"' | b'`' => {
                k = skip_string(rb, k);

                bare.push('S');
            }
            b'\'' if !rust => {
                k = skip_string(rb, k);

                bare.push('S');
            }
            c => {
                bare.push(c as char);
                k += 1;
            }
        }
    }
    let trimmed = bare.trim();
    if trimmed.is_empty() {
        return Some((i, true));
    }
    let scalar = |t: &str| {
        let t = t.trim().trim_start_matches('-');
        t == "S"
            || matches!(t, "true" | "false" | "null" | "None")
            || (!t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_digit() || c == '.' || c == '_')
                && t.chars().next().is_some_and(|c| c.is_ascii_digit()))
    };
    if scalar(trimmed) {
        return Some((i, true));
    }
    // a record: { key: lit, ... } or ( lit, lit ) — keys are words
    let inner = trimmed
        .strip_prefix('{')
        .and_then(|r| r.strip_suffix('}'))
        .or_else(|| trimmed.strip_prefix('(').and_then(|r| r.strip_suffix(')')));
    let record = inner.is_some_and(|inner| {
        !inner.trim().is_empty()
            && inner.split(',').filter(|f| !f.trim().is_empty()).all(|f| {
                let v = f.rsplit_once(':').map_or(f, |(_, v)| v);
                let key_ok = f.rsplit_once(':').is_none_or(|(k, _)| {
                    let k = k.trim();
                    k == "S" || k.chars().all(|c| c.is_alphanumeric() || c == '_')
                });
                key_ok && scalar(v)
            })
    });
    Some((i, record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::rules::Rule;

    fn rules(limit: Option<u32>) -> Rules {
        Rules {
            rules: vec![Rule {
                id: "data-home".into(),
                text: "t".into(),
                check: Some("data-literal".into()),
                values: vec!["web/src/data/**".into()],
                limit,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn lists_of_literals_and_records_are_data() {
        let ts = "const plans = [\n  { name: 'Free', price: 0 },\n  { name: 'Pro', price: 29 },\n  { name: 'Team', price: 99 },\n];\nconst n = [1, 2];\nconst calls = [a(), b()];\n";
        assert_eq!(
            lists(&live_text(ts).0, false)
                .iter()
                .map(|(_, n)| *n)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
        let rs = "const COUNTRIES: &[&str] = &[\"RO\", \"DE\", \"FR\", \"IT\", \"ES\", \"PL\"];\nfn f<'a>(x: &'a [u8]) {}\n";
        assert_eq!(
            lists(&live_text(rs).0, true)
                .iter()
                .map(|(_, n)| *n)
                .collect::<Vec<_>>(),
            vec![6]
        );
    }

    #[test]
    fn a_long_list_outside_the_data_home_is_named() {
        let src =
            "export const COUNTRIES = [\"RO\", \"DE\", \"FR\", \"IT\", \"ES\", \"PL\", \"NL\"];\n";
        let mut out = Vec::new();
        check(&rules(None), None, "web/src/Signup.tsx", src, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].line, 1);
        let mut out = Vec::new();
        check(
            &rules(None),
            None,
            "web/src/data/countries.ts",
            src,
            &mut out,
        );
        assert!(out.is_empty(), "the data home holds data");
        let mut out = Vec::new();
        check(&rules(Some(10)), None, "web/src/Signup.tsx", src, &mut out);
        assert!(out.is_empty(), "under the rule's limit");
    }
}
