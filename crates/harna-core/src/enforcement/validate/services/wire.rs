//! What a field's type is on the wire (0.9.5, books field test case 2d: a
//! Rust `amount_minor: i64` mirrored as a TS `amountMinor: string` passed —
//! only the names were compared across languages).
//!
//! `wire_kind` maps a declared type, Rust or TS, to the JSON it travels as:
//! `number`, `string`, `bool`, `array<…>` or `map<…>`. Wrappers that do not
//! change the wire (`Option`, `Box`, `Arc`, `Rc`, `| null`, `| undefined`,
//! `readonly`) are seen through. A named type harna cannot resolve is
//! `None`: unknown, never reported — a mismatch is claimed only when both
//! sides are known and differ.

/// The JSON kind a declared type travels as, or None when unknown.
pub fn wire_kind(ty: &str, rust: bool) -> Option<String> {
    let t = ty.trim().trim_start_matches("readonly ").trim();
    if t.is_empty() {
        return None;
    }
    if rust {
        rust_kind(
            t.trim_start_matches('&')
                .trim_start_matches("'static ")
                .trim(),
        )
    } else {
        ts_kind(t)
    }
}

fn generic<'a>(t: &'a str, names: &[&str]) -> Option<&'a str> {
    for n in names {
        if let Some(rest) = t.strip_prefix(n) {
            let rest = rest.trim_start();
            if let Some(inner) = rest.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
                return Some(inner.trim());
            }
        }
    }
    None
}

/// The last argument of a two-argument generic (`K, V` -> `V`).
fn second(args: &str) -> &str {
    let mut depth = 0i32;
    for (i, ch) in args.char_indices() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => return args[i + 1..].trim(),
            _ => {}
        }
    }
    args
}

fn rust_kind(t: &str) -> Option<String> {
    let last = t.rsplit("::").next().unwrap_or(t);
    match last {
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" | "f32" | "f64" => return Some("number".into()),
        "String" | "str" | "char" => return Some("string".into()),
        "bool" => return Some("bool".into()),
        _ => {}
    }
    if let Some(inner) = generic(last, &["Option", "Box", "Arc", "Rc", "Cow"]) {
        // Cow<'a, str>: the last argument is the type
        return rust_kind(second(inner).trim_start_matches('&'));
    }
    if let Some(inner) = generic(last, &["Vec", "VecDeque", "HashSet", "BTreeSet"]) {
        return Some(format!("array<{}>", rust_kind(inner).unwrap_or_default()));
    }
    if let Some(inner) = generic(last, &["HashMap", "BTreeMap", "IndexMap"]) {
        return Some(format!(
            "map<{}>",
            rust_kind(second(inner)).unwrap_or_default()
        ));
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let elem = inner.split(';').next().unwrap_or(inner).trim();
        return Some(format!("array<{}>", rust_kind(elem).unwrap_or_default()));
    }
    None
}

fn ts_kind(t: &str) -> Option<String> {
    // `T | null | undefined`: the non-null member is the kind
    let members: Vec<&str> = t
        .split('|')
        .map(str::trim)
        .filter(|m| !m.is_empty() && !matches!(*m, "null" | "undefined"))
        .collect();
    if members.len() != 1 {
        return None; // a real union: say nothing
    }
    let t = members[0];
    match t {
        "number" | "bigint" => return Some("number".into()),
        "string" => return Some("string".into()),
        "boolean" => return Some("bool".into()),
        _ => {}
    }
    if t.starts_with('"') || t.starts_with('\'') || t.starts_with('`') {
        return Some("string".into()); // a string literal type
    }
    if let Some(elem) = t.strip_suffix("[]") {
        let elem = elem.trim().trim_start_matches('(').trim_end_matches(')');
        return Some(format!("array<{}>", ts_kind(elem).unwrap_or_default()));
    }
    if let Some(inner) = generic(t, &["Array", "ReadonlyArray", "Set"]) {
        return Some(format!("array<{}>", ts_kind(inner).unwrap_or_default()));
    }
    if let Some(inner) = generic(t, &["Record", "Map"]) {
        return Some(format!(
            "map<{}>",
            ts_kind(second(inner)).unwrap_or_default()
        ));
    }
    None
}

/// A JSON Schema property's kind: its `type` (`integer` is a number),
/// arrays with their `items`; None for objects, unions and no type.
pub fn json_kind(p: &serde_json::Value) -> Option<String> {
    match p.get("type")?.as_str()? {
        "integer" | "number" => Some("number".into()),
        "string" => Some("string".into()),
        "boolean" => Some("bool".into()),
        "array" => Some(format!(
            "array<{}>",
            p.get("items").and_then(json_kind).unwrap_or_default()
        )),
        _ => None,
    }
}

/// Two kinds disagree only when both are known and differ; an unknown
/// element (`array<>`) matches any element.
pub fn disagree(a: &str, b: &str) -> bool {
    fn split(k: &str) -> (&str, &str) {
        match k.split_once('<') {
            Some((head, rest)) => (head, rest.strip_suffix('>').unwrap_or(rest)),
            None => (k, ""),
        }
    }
    let ((ha, ea), (hb, eb)) = (split(a), split(b));
    if ha != hb {
        return true;
    }
    !ea.is_empty() && !eb.is_empty() && disagree(ea, eb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_and_ts_meet_on_the_wire() {
        assert_eq!(wire_kind("i64", true).as_deref(), Some("number"));
        assert_eq!(wire_kind("number", false).as_deref(), Some("number"));
        assert_eq!(wire_kind("Option<String>", true).as_deref(), Some("string"));
        assert_eq!(wire_kind("string | null", false).as_deref(), Some("string"));
        assert_eq!(
            wire_kind("Vec<u32>", true).as_deref(),
            Some("array<number>")
        );
        assert_eq!(
            wire_kind("readonly number[]", false).as_deref(),
            Some("array<number>")
        );
        assert_eq!(
            wire_kind("std::collections::BTreeMap<String, bool>", true).as_deref(),
            Some("map<bool>")
        );
        assert_eq!(
            wire_kind("Record<string, boolean>", false).as_deref(),
            Some("map<bool>")
        );
        assert_eq!(wire_kind("&'static str", true).as_deref(), Some("string"));
    }

    #[test]
    fn unknown_types_say_nothing() {
        assert_eq!(wire_kind("Money", true), None);
        assert_eq!(wire_kind("Account", false), None);
        assert_eq!(wire_kind("string | number", false), None);
        assert!(!disagree("array<>", "array<number>"));
        assert!(disagree("number", "string"));
        assert!(disagree("array<number>", "array<string>"));
    }
}
