//! Check `corridor-shape` (0.8.2): both ends of a corridor speak the same
//! message. For each message whose shape is a type in the code, every
//! service on the corridor either names that type (it links the shared
//! shape) or declares its own type of the same name — a TS interface
//! mirroring a Rust struct, say — whose fields must be the shape's fields.
//! Field names compare without case or `_` (`posted_at` = `postedAt`, the
//! usual serde rename). A service that never names the message is told
//! (info): its side of the corridor cannot be read.

use std::path::Path;

use super::super::{Severity, Violation};
use crate::schema::model::manifest::Manifest;
use crate::schema::store::services::Services;
use crate::search::digest::Digest;

const SHAPE_KINDS: &[&str] = &["struct", "enum", "type", "interface", "class"];

pub fn check(
    root: &Path,
    services: &Services,
    manifest: &Manifest,
    d: &Digest,
    out: &mut Vec<Violation>,
) {
    let read = |p: &str| std::fs::read_to_string(root.join(p)).unwrap_or_default();
    for c in &services.corridors {
        for msg in &c.messages {
            let Some((file, ty)) = msg.type_ref() else {
                continue; // a schema file: nothing to read here
            };
            let Some(wanted) = fields_of(&read(file), ty) else {
                continue; // the shape itself is coherence's business
            };
            for s in c.from.iter().chain(&c.to) {
                let Some(svc) = services.get(s) else {
                    continue;
                };
                let paths: Vec<&str> = manifest
                    .layers
                    .iter()
                    .flat_map(|l| &l.containers)
                    .filter(|x| svc.containers.contains(&x.name))
                    .map(|x| x.path.as_str())
                    .collect();
                let modules: Vec<&crate::search::digest::Module> = d
                    .modules
                    .iter()
                    .filter(|m| {
                        paths
                            .iter()
                            .any(|p| crate::schema::model::scope::path_in_container(&m.path, p))
                    })
                    .collect();
                // its own declarations of the type
                let own: Vec<&&crate::search::digest::Module> = modules
                    .iter()
                    .filter(|m| {
                        m.path != file
                            && m.inventory
                                .iter()
                                .any(|i| i.name == ty && SHAPE_KINDS.contains(&i.kind.as_str()))
                    })
                    .collect();
                let mut push = |file_: &str, severity: Severity, message: String, hint: &str| {
                    out.push(Violation {
                        check: "corridor-shape".into(),
                        file: file_.into(),
                        line: 0,
                        severity,
                        message,
                        hint: hint.into(),
                    });
                };
                for m in &own {
                    let Some(theirs) = fields_of(&read(&m.path), ty) else {
                        continue;
                    };
                    let key = |f: &String| f.to_lowercase().replace('_', "");
                    let w: Vec<String> = wanted.iter().map(key).collect();
                    let t: Vec<String> = theirs.iter().map(key).collect();
                    let missing: Vec<&String> =
                        wanted.iter().filter(|f| !t.contains(&key(f))).collect();
                    let extra: Vec<&String> =
                        theirs.iter().filter(|f| !w.contains(&key(f))).collect();
                    if !missing.is_empty() || !extra.is_empty() {
                        push(
                            &m.path,
                            crate::enforcement::checks::severity_of("corridor-shape"),
                            format!(
                                "service `{s}`'s `{ty}` differs from corridor `{}`'s shape ({file}): missing [{}], extra [{}]",
                                c.name,
                                missing.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", "),
                                extra.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")
                            ),
                            "make the fields match, or have both sides link the one shape from shared code",
                        );
                    }
                }
                let names_it = modules.iter().any(|m| {
                    m.path == file
                        || crate::enforcement::parity::policy::names_word(&read(&m.path), ty)
                });
                if own.is_empty() && !names_it {
                    push(
                        ".harna/services.toml",
                        Severity::Info,
                        format!(
                            "service `{s}` is on corridor `{}` but its code never names `{ty}`: its side of `{}` cannot be read",
                            c.name, msg.name
                        ),
                        "send or receive the message by its declared type, or fix the corridor's services",
                    );
                }
            }
        }
    }
}

/// The field names of a struct / interface / class / object type named
/// `ty` in `src` (Rust or TS), in order; None when it is not declared or
/// has no braced body (a tuple struct, an alias of another type).
pub fn fields_of(src: &str, ty: &str) -> Option<Vec<String>> {
    let decl = regex::Regex::new(&format!(
        r"\b(?:struct|interface|class|type)\s+{}\b[^{{;]*\{{",
        regex::escape(ty)
    ))
    .ok()?;
    let m = decl.find(src)?;
    let body_start = m.end();
    let mut depth = 1i32;
    let mut end = body_start;
    let mut prev = ' ';
    for (i, ch) in src[body_start..].char_indices() {
        let arrow = ch == '>' && (prev == '=' || prev == '-');
        prev = ch;
        match ch {
            '{' | '(' | '[' | '<' => depth += 1,
            '}' | ')' | ']' | '>' if !arrow => {
                depth -= 1;
                if depth == 0 {
                    end = body_start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &src[body_start..end];
    // top-level pieces only: split on , ; and newlines at depth 0
    let mut fields = Vec::new();
    let mut piece = String::new();
    let mut depth = 0i32;
    let field = regex::Regex::new(
        r"^(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+|readonly\s+|public\s+|private\s+|protected\s+)*([A-Za-z_$][\w$]*)\s*\??\s*:",
    )
    .ok()?;
    let mut flush = |piece: &mut String| {
        let t = piece.trim();
        if let Some(c) = field.captures(t) {
            fields.push(c[1].to_string());
        }
        piece.clear();
    };
    for line in body.lines() {
        let line = line.split("//").next().unwrap_or("");
        let mut prev = ' ';
        for ch in line.chars() {
            let arrow = ch == '>' && (prev == '=' || prev == '-');
            prev = ch;
            match ch {
                '{' | '(' | '[' | '<' => depth += 1,
                '}' | ')' | ']' | '>' if !arrow => depth -= 1,
                _ => {}
            }
            if depth == 0 && (ch == ',' || ch == ';') {
                flush(&mut piece);
            } else {
                piece.push(ch);
            }
        }
        if depth == 0 {
            flush(&mut piece);
        } else {
            piece.push(' ');
        }
    }
    flush(&mut piece);
    Some(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::map::Item;
    use crate::schema::model::manifest::{Container, Layer};
    use crate::schema::store::services::{Corridor, Message, Service};
    use crate::search::digest::Module;

    #[test]
    fn fields_read_from_rust_and_ts() {
        let rs = "#[derive(Serialize)]\npub struct Posted {\n    pub id: u64,\n    #[serde(default)]\n    pub posted_at: Option<Vec<(u8, u8)>>,\n    amount: Money,\n}\n";
        assert_eq!(
            fields_of(rs, "Posted").unwrap(),
            vec!["id", "posted_at", "amount"]
        );
        let ts = "export interface Posted {\n  id: number;\n  postedAt?: string; // when\n  meta: { a: number, b: string };\n  done: (x: number) => void;\n}\n";
        assert_eq!(
            fields_of(ts, "Posted").unwrap(),
            vec!["id", "postedAt", "meta", "done"]
        );
        assert!(fields_of("pub struct Posted(u64);", "Posted").is_none());
    }

    /// 0.8.2: the Rust sender links the shape; the TS receiver's own
    /// `Posted` lacks `amount` and adds `note` — named, case-insensitively
    /// matching `posted_at`/`postedAt`.
    #[test]
    fn corridor_ends_speak_the_same_shape() {
        let root = std::env::temp_dir().join(format!("harna-shapes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(
            "crates/events/src/lib.rs",
            "pub struct Posted {\n    pub id: u64,\n    pub posted_at: String,\n    pub amount: i64,\n}\n",
        );
        w(
            "crates/api/src/lib.rs",
            "use events::Posted;\nfn send(p: Posted) {}\n",
        );
        w(
            "web/src/types.ts",
            "export interface Posted { id: number; postedAt: string; note: string }\n",
        );
        let c = |n: &str, p: &str| Container {
            name: n.into(),
            path: p.into(),
            ..Default::default()
        };
        let m = Manifest {
            layers: vec![Layer {
                name: "all".into(),
                containers: vec![
                    c("events", "crates/events/src"),
                    c("api", "crates/api/src"),
                    c("web", "web/src"),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let item = |n: &str, k: &str| Item {
            vis: "pub".into(),
            kind: k.into(),
            name: n.into(),
            text: String::new(),
        };
        let d = Digest {
            modules: vec![
                Module {
                    path: "crates/events/src/lib.rs".into(),
                    inventory: vec![item("Posted", "struct")],
                    ..Default::default()
                },
                Module {
                    path: "crates/api/src/lib.rs".into(),
                    ..Default::default()
                },
                Module {
                    path: "web/src/types.ts".into(),
                    inventory: vec![item("Posted", "interface")],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let svc = |n: &str| Service {
            name: n.into(),
            containers: vec![n.into()],
            ..Default::default()
        };
        let s = Services {
            services: vec![svc("api"), svc("web")],
            corridors: vec![Corridor {
                name: "postings".into(),
                from: vec!["api".into()],
                to: vec!["web".into()],
                messages: vec![Message::parse("crates/events/src/lib.rs::Posted").unwrap()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut out = Vec::new();
        check(&root, &s, &m, &d, &mut out);
        assert_eq!(out.len(), 1, "{out:#?}");
        assert_eq!(out[0].file, "web/src/types.ts");
        assert!(
            out[0].message.contains("missing [amount], extra [note]"),
            "{}",
            out[0].message
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
