//! Reading a message shape's fields from source (0.8.2, rewritten 0.8.4
//! after the 0.8 review and the books field test).
//!
//! `fields_of` finds `struct` / `interface` / `class` / `type X = {…}` named
//! `ty` and returns its fields with their declared types, in order. Comments
//! and string/char literal contents are blanked first, so a `>` in a doc
//! comment or a `"}"` in an attribute cannot cut the body. The declaration
//! must start a line (after `pub` / `export` / `declare` / …), so a mention
//! in prose is not a declaration. Generics and `extends Base<{…}>` are
//! skipped to the real body; a TS `extends` of a type declared in the same
//! file contributes its fields. TS modifiers (`readonly`, `declare`,
//! `public`, `!`, `?`), `#private` and constructor parameter properties are
//! read; `static` members and methods are not fields. Rust `r#type` reads
//! as `type`. Enums and tuple structs have no named fields: None.

use std::collections::BTreeSet;

/// A field: its name and its declared type text (whitespace collapsed).
pub type Field = (String, String);

pub fn fields_of(src: &str, ty: &str, rust: bool) -> Option<Vec<Field>> {
    let clean = blank(src, rust);
    fields_in(&clean, ty, &mut BTreeSet::new())
}

fn fields_in(clean: &str, ty: &str, seen: &mut BTreeSet<String>) -> Option<Vec<Field>> {
    if !seen.insert(ty.to_string()) {
        return None; // a cycle of extends
    }
    let head = regex::Regex::new(&format!(
        r"(?m)^[ \t]*(?:(?:pub(?:\([^)]*\))?|export|default|declare|abstract)\s+)*(?:struct|interface|class|type)\s+{}\b",
        regex::escape(ty)
    ))
    .ok()?;
    let m = head.find(clean)?;
    // from the name to the body's `{`, outside generics; `;` or `(` at
    // depth 0 first means no braced body (alias, tuple or unit struct)
    let rest = &clean[m.end()..];
    let mut angle = 0i32;
    let mut prev = ' ';
    let mut open = None;
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
            ';' | '(' if angle <= 0 => return None,
            _ => {}
        }
    }
    let open = open?;
    let between = &rest[..open];
    let body_start = m.end() + open + 1;
    let mut depth = 1i32;
    let mut end = None;
    let mut prev = ' ';
    for (i, ch) in clean[body_start..].char_indices() {
        let arrow = ch == '>' && (prev == '=' || prev == '-');
        prev = ch;
        match ch {
            '{' | '(' | '[' | '<' => depth += 1,
            '}' | ')' | ']' | '>' if !arrow => {
                depth -= 1;
                if depth == 0 {
                    end = Some(body_start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &clean[body_start..end?];
    let mut fields: Vec<Field> = Vec::new();
    // TS `extends A, B` (interfaces) / `extends A` (classes): inherited
    // fields first, when their declaration is in this file
    if let Some(ext) = between.split("extends").nth(1) {
        let ext = ext.split("implements").next().unwrap_or(ext);
        for base in top_level(ext, ',') {
            let name: String = base
                .trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if !name.is_empty() {
                if let Some(inherited) = fields_in(clean, &name, seen) {
                    fields.extend(inherited);
                }
            }
        }
    }
    let field = regex::Regex::new(
        r"^(?:#\[[^\]]*\]\s*)*((?:(?:pub(?:\([^)]*\))?|readonly|public|private|protected|declare|static|override|abstract)\s+)*)(r#)?(#?[A-Za-z_$][\w$]*)\s*[?!]?\s*:(.*)$",
    )
    .ok()?;
    let param = regex::Regex::new(
        r"(?:public|private|protected|readonly)\s+(?:readonly\s+)?([A-Za-z_$][\w$]*)\s*[?!]?\s*:([^,]*)",
    )
    .ok()?;
    for piece in top_level_pieces(body) {
        let t = piece.trim();
        if let Some(args) = t.strip_prefix("constructor") {
            for c in param.captures_iter(args) {
                fields.push((c[1].to_string(), squash(&c[2])));
            }
            continue;
        }
        if let Some(c) = field.captures(t) {
            if c[1].split_whitespace().any(|w| w == "static") {
                continue;
            }
            let ty_text = c[4].trim().trim_end_matches(',').trim_end_matches(';');
            fields.push((c[3].to_string(), squash(ty_text)));
        }
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

/// A body's members: split at `,` / `;` / line ends outside brackets.
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
    out.into_iter().filter(|p| !p.trim().is_empty()).collect()
}

/// Comments removed and string/char literal contents blanked (the quotes
/// stay); line structure kept. In Rust a `'` opens a char literal only as
/// `'x'` or `'\…'` — otherwise it is a lifetime.
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
                if chars.get(i) == Some(&'\n') {
                    out.push('\n');
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

    fn names(src: &str, ty: &str, rust: bool) -> Vec<String> {
        fields_of(src, ty, rust)
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect()
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

    #[test]
    fn ts_fields_modifiers_extends_and_constructors() {
        let ts = "export interface Base { id: number }\nexport interface Posted extends Base {\n  postedAt?: string; // id >= 1\n  readonly amount!: number;\n  meta: { a: number, b: string };\n  done: (x: number) => void;\n  kind: 'a' | '}';\n}\n";
        assert_eq!(
            names(ts, "Posted", false),
            vec!["id", "postedAt", "amount", "meta", "done", "kind"]
        );
        let class = "export class Posted extends Model<{ a: number }> {\n  static count: number = 0;\n  declare id: number;\n  #secret: string;\n  constructor(public account: string, private readonly amountMinor: number) { super(); }\n  total(): number { return 1; }\n}\n";
        assert_eq!(
            names(class, "Posted", false),
            vec!["id", "#secret", "account", "amountMinor"]
        );
        let alias = "type Posted = {\n  id: number;\n  note: string;\n};\n";
        assert_eq!(names(alias, "Posted", false), vec!["id", "note"]);
    }
}
