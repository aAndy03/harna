//! Reading a message shape's fields from source (0.8.2, rewritten 0.8.4
//! after the 0.8 review and the books field test, extended 0.9.2 after the
//! TS field test).
//!
//! `fields_of` finds `struct` / `interface` / `class` / `type X = {…}` named
//! `ty` and returns its fields with their declared types, in order. Comments
//! are dropped and string contents defused (brackets and separators
//! blanked, words kept), so a `>` in a doc comment or a `"}"` in an
//! attribute cannot cut the body while `#[serde(rename = "postedAt")]`
//! still reads. The declaration must start a line (after `pub` / `export` /
//! `declare` / …), so a mention in prose is not a declaration. Generics and
//! `extends Base<{…}>` are skipped to the real body. Bases contribute their
//! fields: `extends A, B` and `type X = A & B & {…}`, found in the same file
//! or through `lookup` (another file, a workspace package). TS modifiers
//! (`readonly`, `declare`, `public`, `!`, `?`), `#private`, quoted keys,
//! untyped class initializers (`at = 0`) and constructor parameter
//! properties are read; `static` members and methods are not fields. Rust
//! `r#type` reads as `type`; serde's `rename` names the field on the wire,
//! `skip` / `skip_serializing` leave it out. Enums and tuple structs have no
//! named fields: None.

use std::collections::BTreeSet;

/// A field: its name and its declared type text (whitespace collapsed;
/// empty when the source gives none, as an untyped initializer).
pub type Field = (String, String);

/// The source of another file declaring a type name (for bases declared
/// elsewhere), or None.
pub type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

pub fn fields_of(src: &str, ty: &str, rust: bool) -> Option<Vec<Field>> {
    fields_of_with(src, ty, rust, &|_| None)
}

pub fn fields_of_with(src: &str, ty: &str, rust: bool, lookup: Lookup) -> Option<Vec<Field>> {
    let clean = blank(src, rust);
    fields_in(&clean, ty, rust, lookup, &mut BTreeSet::new(), 0)
}

/// A base's fields: in this file, else through `lookup`.
fn base_fields(
    clean: &str,
    name: &str,
    rust: bool,
    lookup: Lookup,
    seen: &mut BTreeSet<String>,
    depth: usize,
) -> Option<Vec<Field>> {
    fields_in(clean, name, rust, lookup, seen, depth + 1).or_else(|| {
        let other = lookup(name)?;
        let other = blank(&other, rust);
        seen.remove(name);
        fields_in(&other, name, rust, lookup, seen, depth + 1)
    })
}

/// The type names a list of bases spells (`A, B<T>`, `A & B`).
fn base_names(text: &str, sep: char) -> Vec<String> {
    top_level(text, sep)
        .iter()
        .map(|b| {
            b.trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect::<String>()
        })
        .filter(|n| !n.is_empty())
        .collect()
}

fn fields_in(
    clean: &str,
    ty: &str,
    rust: bool,
    lookup: Lookup,
    seen: &mut BTreeSet<String>,
    depth: usize,
) -> Option<Vec<Field>> {
    if depth > 8 || !seen.insert(ty.to_string()) {
        return None; // a cycle of bases, or a chain too deep to be data
    }
    let head = regex::Regex::new(&format!(
        r"(?m)^[ \t]*(?:#\[[^\]]*\][ \t]*)*(?:(?:pub(?:\([^)]*\))?|export|default|declare|abstract)\s+)*(?:struct|interface|class|type)\s+{}\b",
        regex::escape(ty)
    ))
    .ok()?;
    let Some(m) = head.find(clean) else {
        seen.remove(ty); // not here: another file may declare it
        return None;
    };
    // from the name to the body's `{`, outside generics; `;` or `(` at
    // depth 0 first means no braced body (alias, tuple or unit struct)
    let rest = &clean[m.end()..];
    let mut angle = 0i32;
    let mut prev = ' ';
    let mut open = None;
    let mut stop = rest.len();
    for (i, ch) in rest.char_indices() {
        let arrow = ch == '>' && (prev == '=' || prev == '-');
        prev = ch;
        match ch {
            '<' => angle += 1,
            '>' if !arrow => angle -= 1,
            '{' if angle <= 0 => {
                open = Some(i);
                break;
            }
            ';' | '(' if angle <= 0 => {
                stop = i;
                break;
            }
            _ => {}
        }
    }
    let between = &rest[..open.unwrap_or(stop)];
    let mut fields: Vec<Field> = Vec::new();
    // bases: `extends A, B` (interfaces, classes) and `= A & B & {…}`
    let mut bases: Vec<String> = Vec::new();
    if let Some(ext) = between.split("extends").nth(1) {
        let ext = ext.split("implements").next().unwrap_or(ext);
        bases.extend(base_names(ext, ','));
    }
    if let Some((_, alias)) = between.split_once('=') {
        bases.extend(base_names(alias, '&'));
    }
    let mut resolved_any = false;
    for b in &bases {
        if let Some(inherited) = base_fields(clean, b, rust, lookup, seen, depth) {
            resolved_any = true;
            fields.extend(inherited);
        }
    }
    let Some(open) = open else {
        // `type X = A & B;`: the bases are the shape
        return resolved_any.then_some(fields);
    };
    let body_start = m.end() + open + 1;
    let mut depth_b = 1i32;
    let mut end = None;
    let mut prev = ' ';
    for (i, ch) in clean[body_start..].char_indices() {
        let arrow = ch == '>' && (prev == '=' || prev == '-');
        prev = ch;
        match ch {
            '{' | '(' | '[' | '<' => depth_b += 1,
            '}' | ')' | ']' | '>' if !arrow => {
                depth_b -= 1;
                if depth_b == 0 {
                    end = Some(body_start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &clean[body_start..end?];
    let field = regex::Regex::new(
        r#"^((?:#\[[^\]]*\]\s*)*)((?:(?:pub(?:\([^)]*\))?|readonly|public|private|protected|declare|static|override|abstract)\s+)*)(?:"([^"]+)"|'([^']+)'|(r#)?(#?[A-Za-z_$][\w$]*))\s*[?!]?\s*(:|=)(.*)$"#,
    )
    .ok()?;
    let param = regex::Regex::new(
        r"(?:public|private|protected|readonly)\s+(?:readonly\s+)?([A-Za-z_$][\w$]*)\s*[?!]?\s*:([^,]*)",
    )
    .ok()?;
    let rename = regex::Regex::new(r#"rename\s*=\s*"\s*([^"]*?)\s*""#).ok()?;
    for piece in top_level_pieces(body) {
        let t = piece.trim();
        if let Some(args) = t.strip_prefix("constructor") {
            for c in param.captures_iter(args) {
                fields.push((c[1].to_string(), squash(&c[2])));
            }
            continue;
        }
        let Some(c) = field.captures(t) else {
            continue;
        };
        let attrs = &c[1];
        if c[2].split_whitespace().any(|w| w == "static") {
            continue;
        }
        // `=` only in a TS class (an initializer); a Rust `=` is not a field
        let untyped = &c[7] == "=";
        if untyped && rust {
            continue;
        }
        // serde on the wire (0.9.2): skipped fields are not sent; a rename
        // is the name the other end sees
        if rust && attrs.contains("serde") {
            let skipped = attrs
                .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .any(|w| matches!(w, "skip" | "skip_serializing" | "skip_deserializing"));
            if skipped || attrs.contains("flatten") {
                continue;
            }
        }
        let mut name = c
            .get(3)
            .or_else(|| c.get(4))
            .or_else(|| c.get(6))
            .map(|n| n.as_str().to_string())
            .unwrap_or_default();
        if rust {
            if let Some(r) = rename.captures(attrs) {
                if !r[1].is_empty() {
                    name = r[1].to_string();
                }
            }
        }
        let ty_text = if untyped {
            String::new()
        } else {
            squash(c[8].trim().trim_end_matches(',').trim_end_matches(';'))
        };
        fields.push((name, ty_text));
    }
    Some(fields)
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split at `sep` outside brackets.
fn top_level(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut prev = ' ';
    for ch in s.chars() {
        let arrow = ch == '>' && (prev == '=' || prev == '-');
        prev = ch;
        match ch {
            '{' | '(' | '[' | '<' => depth += 1,
            '}' | ')' | ']' | '>' if !arrow => depth -= 1,
            _ => {}
        }
        if depth == 0 && ch == sep {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out
}

/// A body's members: split at `,` / `;` / line ends outside brackets; an
/// attribute on its own line belongs to the member after it.
fn top_level_pieces(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut prev = ' ';
    for ch in body.chars() {
        let arrow = ch == '>' && (prev == '=' || prev == '-');
        prev = ch;
        match ch {
            '{' | '(' | '[' | '<' => depth += 1,
            '}' | ')' | ']' | '>' if !arrow => depth -= 1,
            _ => {}
        }
        if depth == 0 && (ch == ',' || ch == ';' || ch == '\n') {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(if ch == '\n' { ' ' } else { ch });
        }
    }
    out.push(cur);
    let mut joined: Vec<String> = Vec::new();
    let mut pending = String::new();
    for p in out {
        let t = p.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("#[") && t.ends_with(']') {
            pending.push_str(t);
            pending.push(' ');
            continue;
        }
        joined.push(format!("{pending}{t}"));
        pending.clear();
    }
    joined
}

/// Comments removed and string/char literal contents defused: words and
/// `_ - . $ @` kept (so a serde `rename` value reads), every other
/// character blanked (so no bracket or separator inside a string cuts a
/// body); quotes and line structure kept. In Rust a `'` opens a char
/// literal only as `'x'` or `'\…'` — otherwise it is a lifetime.
pub fn blank(src: &str, rust: bool) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                if chars[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 2;
            out.push(' ');
            continue;
        }
        let quote = match c {
            '"' | '`' => true,
            '\'' if !rust => true,
            '\'' => next == Some('\\') || chars.get(i + 2) == Some(&'\''),
            _ => false,
        };
        if quote {
            out.push(c);
            i += 1;
            while i < chars.len() && chars[i] != c {
                if chars[i] == '\\' {
                    i += 1;
                }
                match chars.get(i) {
                    Some('\n') => out.push('\n'),
                    Some(ch) if ch.is_alphanumeric() || "_-.$@".contains(*ch) => out.push(*ch),
                    Some(_) => out.push(' '),
                    None => {}
                }
                i += 1;
            }
            out.push(c);
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names_with(src: &str, ty: &str, rust: bool, lookup: Lookup) -> Vec<String> {
        fields_of_with(src, ty, rust, lookup)
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect()
    }

    fn names(src: &str, ty: &str, rust: bool) -> Vec<String> {
        names_with(src, ty, rust, &|_| None)
    }

    #[test]
    fn rust_fields_survive_comments_strings_and_attributes() {
        let rs = "#[derive(Serialize)]\npub struct Posted {\n    /// amount > 0 (< 1e9)\n    pub id: u64,\n    #[serde(rename = \"}\")]\n    pub posted_at: Option<Vec<(u8, u8)>>,\n    pub r#type: String,\n    amount: Money<'a>,\n}\n";
        assert_eq!(
            names(rs, "Posted", true),
            vec!["id", "posted_at", "type", "amount"]
        );
        let typed = fields_of(rs, "Posted", true).unwrap();
        assert_eq!(typed[0].1, "u64");
        assert!(fields_of("pub struct Posted(u64);", "Posted", true).is_none());
        // a mention in prose is not the declaration
        let prose = "// see struct Posted { x }\npub struct Posted {\n    pub y: u8,\n}\n";
        assert_eq!(names(prose, "Posted", true), vec!["y"]);
        assert!(fields_of("pub struct Posted {\n pub a: u8,\n", "Posted", true).is_none());
    }

    /// 0.9.2: serde renames name the wire field; skipped fields are not sent.
    #[test]
    fn serde_names_the_wire() {
        let rs = "#[derive(Serialize)]\npub struct Posted {\n    #[serde(rename = \"postedAt\")]\n    pub at: String,\n    #[serde(skip)]\n    pub cache: u8,\n    #[serde(skip_serializing_if = \"Option::is_none\")]\n    pub note: Option<String>,\n}\n";
        assert_eq!(names(rs, "Posted", true), vec!["postedAt", "note"]);
    }

    #[test]
    fn ts_fields_modifiers_extends_and_constructors() {
        let ts = "export interface Base { id: number }\nexport interface Posted extends Base {\n  postedAt?: string; // id >= 1\n  readonly amount!: number;\n  meta: { a: number, b: string };\n  done: (x: number) => void;\n  kind: 'a' | '}';\n  \"quoted-key\": number;\n}\n";
        assert_eq!(
            names(ts, "Posted", false),
            vec![
                "id",
                "postedAt",
                "amount",
                "meta",
                "done",
                "kind",
                "quoted-key"
            ]
        );
        let class = "export class Posted extends Model<{ a: number }> {\n  static count: number = 0;\n  declare id: number;\n  #secret: string;\n  at = 0;\n  constructor(public account: string, private readonly amountMinor: number) { super(); }\n  total(): number { return 1; }\n}\n";
        assert_eq!(
            names(class, "Posted", false),
            vec!["id", "#secret", "at", "account", "amountMinor"]
        );
        let alias = "type Posted = {\n  id: number;\n  note: string;\n};\n";
        assert_eq!(names(alias, "Posted", false), vec!["id", "note"]);
    }

    /// 0.9.2: bases declared in another file, and intersections.
    #[test]
    fn bases_from_other_files_and_intersections() {
        let base = "export interface Stamped { id: number; at: string }\n";
        let lookup = |n: &str| (n == "Stamped").then(|| base.to_string());
        let ts = "import { Stamped } from '@app/shared';\nexport interface Posted extends Stamped { amount: number }\n";
        assert_eq!(
            names_with(ts, "Posted", false, &lookup),
            vec!["id", "at", "amount"]
        );
        let inter = "export type Posted = Stamped & { amount: number };\n";
        assert_eq!(
            names_with(inter, "Posted", false, &lookup),
            vec!["id", "at", "amount"]
        );
        let pure =
            "export type Posted = Stamped & Extra;\nexport interface Extra { amount: number }\n";
        assert_eq!(
            names_with(pure, "Posted", false, &lookup),
            vec!["id", "at", "amount"]
        );
    }
}
