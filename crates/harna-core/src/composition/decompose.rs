//! Component `decompose` (0.3.31): the split planner — structure born
//! correct, retro-split made mechanical.
//!
//! Given a source file, the planner maps its top-level items (name,
//! kind, line span) and recommends HOW to split it, not just THAT:
//!
//! - **dispatch-split**: one item covers >50% of the file (the giant
//!   `match` over a command enum harna itself grew twice). The `*Cmd`
//!   enums in the file are the split units — each variant's handler
//!   plus its enum moves into its own module, one line of dispatch
//!   stays behind.
//! - **prefix-cluster**: items sharing a name stem (first snake_case
//!   segment) form natural modules.
//!
//! The plan also sketches the manifest patch (new component entries),
//! because an unregistered split is a `manifest-path` violation waiting
//! to happen. Application stays manual — moving code is the agent's
//! judgment call, the planner just makes the cut lines visible. Spans
//! are approximate (comment/string-aware brace counting, not a parser)
//! and say so when they cannot be exact.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ItemSpan {
    pub name: String,
    pub kind: String,
    /// 1-based, attributes included.
    pub start: usize,
    pub end: usize,
}

impl ItemSpan {
    pub fn lines(&self) -> usize {
        self.end.saturating_sub(self.start) + 1
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DecomposePlan {
    pub file: String,
    pub total_lines: usize,
    pub items: Vec<ItemSpan>,
    pub recommendation: String,
    pub manifest_sketch: String,
}

const KEYWORDS: &[&str] = &[
    "fn", "struct", "enum", "trait", "impl", "mod", "type", "const", "static",
];

/// Top-level items with approximate spans. Comment- and string-aware
/// brace counting keeps string literals (`"{"`) and `//` comments from
/// corrupting the depth; block comments and macros are NOT understood —
/// spans near them may be off by a line, the cut still lands visibly.
pub fn item_spans(src: &str) -> Vec<ItemSpan> {
    let lines: Vec<&str> = src.lines().collect();
    let cleaned: Vec<String> = lines.iter().map(|l| strip_code_noise(l)).collect();
    // Depth AFTER each line.
    let mut after: Vec<i32> = Vec::with_capacity(lines.len());
    let mut depth: i32 = 0;
    for c in &cleaned {
        depth += c.chars().filter(|&ch| ch == '{').count() as i32
            - c.chars().filter(|&ch| ch == '}').count() as i32;
        after.push(depth);
    }

    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if after[i] < 0 {
            after[i] = 0; // unbalanced closers (advisory scan, never a crash)
        }
        let base = if i == 0 { 0 } else { after[i - 1].max(0) };
        if base == 0 {
            if let Some((kind, name)) = item_head(lines[i]) {
                // Attributes ride with the item.
                let mut start = i;
                while start > 0 {
                    let t = lines[start - 1].trim_start();
                    if t.starts_with("#[") || t.starts_with("///") {
                        start -= 1;
                    } else {
                        break;
                    }
                }
                let mut end = i;
                if after[i] != base {
                    // Body opens: ends where depth returns to base.
                    let mut j = i + 1;
                    while j < lines.len() && after[j] != base {
                        j += 1;
                    }
                    end = j.min(lines.len().saturating_sub(1));
                }
                items.push(ItemSpan {
                    name,
                    kind: kind.into(),
                    start: start + 1,
                    end: end + 1,
                });
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    items
}

/// Strip `//` comments and `"..."` contents (escapes honored) so braces
/// inside them never count. Single-line approximation, stated as such.
fn strip_code_noise(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '/' if chars.peek() == Some(&'/') => break,
            _ => out.push(c),
        }
    }
    out
}

/// (kind, name) when a depth-0 line opens a top-level item.
fn item_head(line: &str) -> Option<(&'static str, String)> {
    let mut rest = line.trim_start();
    // Visibility and qualifiers come off first.
    loop {
        if let Some(r) = rest.strip_prefix("pub") {
            rest = r.trim_start();
            if rest.starts_with('(') {
                // pub(...) — skip to the matching close.
                let mut depth = 0;
                let mut idx = None;
                for (k, ch) in rest.char_indices() {
                    if ch == '(' {
                        depth += 1;
                    } else if ch == ')' {
                        depth -= 1;
                        if depth == 0 {
                            idx = Some(k + 1);
                            break;
                        }
                    }
                }
                rest = rest[idx.unwrap_or(rest.len())..].trim_start();
            }
            continue;
        }
        let mut ate = false;
        for q in ["unsafe", "async", "extern"] {
            if rest == q || rest.starts_with(&format!("{q} ")) {
                rest = rest[q.len()..].trim_start();
                ate = true;
            }
        }
        if !ate {
            break;
        }
    }
    let found = KEYWORDS.iter().find(|k| {
        let k: &str = k;
        rest == k
            || rest.starts_with(&format!("{k} "))
            || rest.starts_with(&format!("{k}<"))
            || rest.starts_with(&format!("{k}("))
    })?;
    let keyword: &str = found;
    let after_kw = rest[keyword.len()..].trim_start();
    let name = if keyword == "impl" {
        // "impl X" or "impl Trait for X": the type is the last word.
        let tail = after_kw.split('{').next().unwrap_or("").trim();
        let tail = tail.trim_end_matches(';').trim();
        tail.rsplit(" for ")
            .next()
            .unwrap_or(tail)
            .split_whitespace()
            .next_back()
            .unwrap_or("?")
            .trim_matches(|c| c == '<' || c == '>')
            .split('<')
            .next()
            .unwrap_or("?")
            .rsplit("::")
            .next()
            .unwrap_or("?")
            .to_string()
    } else {
        after_kw
            .split(|c| c == '(' || c == '<' || c == '{' || c == ';' || c == ':' || c == ' ')
            .next()
            .unwrap_or("?")
            .trim()
            .to_string()
    };
    if name.is_empty() || name == "?" {
        return None;
    }
    Some((keyword, name))
}

pub fn plan(file: &str, src: &str) -> DecomposePlan {
    let total_lines = src.lines().count().max(1);
    let items = item_spans(src);
    let (recommendation, manifest_sketch) = recommend(file, total_lines, &items);
    DecomposePlan {
        file: file.into(),
        total_lines,
        items,
        recommendation,
        manifest_sketch,
    }
}

impl DecomposePlan {
    pub fn render(&self) -> String {
        let mut out = format!(
            "decompose `{}` ({} lines, {} top-level items — spans approximate):\n",
            self.file,
            self.total_lines,
            self.items.len()
        );
        let mut sorted = self.items.clone();
        sorted.sort_by(|a, b| b.lines().cmp(&a.lines()));
        for it in sorted.iter().take(12) {
            out.push_str(&format!(
                "  {:>5} lines  {:>6}  {}  ({}–{})\n",
                it.lines(),
                it.kind,
                it.name,
                it.start,
                it.end
            ));
        }
        if self.items.len() > 12 {
            out.push_str(&format!("  … +{} more items\n", self.items.len() - 12));
        }
        out.push_str(&format!("\nrecommendation: {}\n", self.recommendation));
        out.push_str(&format!("\nmanifest sketch:\n{}", self.manifest_sketch));
        out
    }
}

fn recommend(file: &str, total: usize, items: &[ItemSpan]) -> (String, String) {
    let dir = std::path::Path::new(file)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| ".".into());
    let stem = |name: &str| {
        name.trim_end_matches("Cmd")
            .trim_end_matches("Command")
            .chars()
            .enumerate()
            .map(|(i, c)| if c.is_uppercase() && i > 0 { format!("_{}", c.to_lowercase()) } else { c.to_lowercase().to_string() })
            .collect::<String>()
    };

    // 1) One dominant item: the dispatch file.
    if let Some(big) = items.iter().max_by_key(|i| i.lines()) {
        if big.lines() * 2 > total {
            let units: Vec<&str> = items
                .iter()
                .filter(|i| {
                    i.kind == "enum"
                        && (i.name.ends_with("Cmd")
                            || i.name.ends_with("Command")
                            || i.name.ends_with("Args"))
                        // Bare `Command`/`Cmd` is the dispatch itself, not a
                        // split unit — its stem is empty, it stays behind.
                        && !["Command", "Cmd", "Args"].contains(&i.name.as_str())
                })
                .map(|i| i.name.as_str())
                .collect();
            if !units.is_empty() {
                let mut rec = format!(
                    "dispatch-split: `{}` ({} lines, {}% of the file) is one dispatch — \
                     move each variant's handler plus its enum into its own module, keep a one-line dispatch behind",
                    big.name,
                    big.lines(),
                    big.lines() * 100 / total
                );
                let mut sketch = String::new();
                for u in &units {
                    let module = stem(u);
                    rec.push_str(&format!("\n  - {u} -> {dir}/{module}.rs"));
                    sketch.push_str(&format!(
                        "[[layers.containers.components]]\nname = \"{module}\"\nkind = \"primitive\"\npath = \"{dir}/{module}.rs\"\nsummary = \"...\"\ntests = [\"{dir}/{module}.rs\"]\n\n"
                    ));
                }
                return (rec, sketch);
            }
        }
    }

    // 2) Prefix clusters: shared name stems are nascent modules.
    let mut groups: std::collections::BTreeMap<String, Vec<&str>> =
        std::collections::BTreeMap::new();
    for it in items {
        if it.kind == "impl" {
            continue;
        }
        let snake: String = it
            .name
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if c.is_uppercase() && i > 0 {
                    format!("_{}", c.to_lowercase())
                } else {
                    c.to_lowercase().to_string()
                }
            })
            .collect();
        let first = snake.split('_').next().unwrap_or("").to_string();
        if first.len() > 2 {
            groups.entry(first).or_default().push(it.name.as_str());
        }
    }
    let clusters: Vec<(&String, &Vec<&str>)> =
        groups.iter().filter(|(_, v)| v.len() >= 2).collect();
    if !clusters.is_empty() {
        let mut rec = String::from("prefix-cluster: shared name stems are nascent modules");
        let mut sketch = String::new();
        for (stem_name, members) in clusters {
            rec.push_str(&format!("\n  - {stem_name} <- {}", members.join(", ")));
            sketch.push_str(&format!(
                "[[layers.containers.components]]\nname = \"{stem_name}\"\nkind = \"primitive\"\npath = \"{dir}/{stem_name}.rs\"\nsummary = \"...\"\ntests = [\"{dir}/{stem_name}.rs\"]\n\n"
            ));
        }
        return (rec, sketch);
    }

    (
        "no dominant item and no shared stems — split by concern manually, one engine per module, register each in the manifest".into(),
        "(no sketch: the cut lines are a judgment call)\n".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"//! Module docs.

use std::path::Path;

/// First engine.
pub struct Engine {
    // a brace in a comment { must not count
    x: u8,
}

impl Engine {
    pub fn run(&self) -> u8 {
        let s = "a { string"; // nor this
        self.x
    }
}

/// Second engine.
pub enum Mode {
    Fast,
    Slow,
}
"#;

    #[test]
    fn spans_name_kind_and_cover_bodies() {
        let items = item_spans(SAMPLE);
        let kinds: Vec<(&str, &str)> = items
            .iter()
            .map(|i| (i.kind.as_str(), i.name.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("struct", "Engine"),
                ("impl", "Engine"),
                ("enum", "Mode"),
            ],
            "{items:#?}"
        );
        assert!(items[0].lines() >= 4, "the struct body is covered");
        assert!(items[1].start > items[0].end, "no overlap");
    }

    #[test]
    fn a_dominant_dispatch_recommends_split_units() {
        let mut src = String::from("enum TermsCmd {\n    Add,\n}\n\nenum Other {\n    X,\n}\n\nfn main() {\n");
        for _ in 0..60 {
            src.push_str("    println!(\"x\");\n");
        }
        src.push_str("}\n");
        let p = plan("crates/x/src/main.rs", &src);
        assert!(
            p.recommendation.starts_with("dispatch-split"),
            "{}",
            p.recommendation
        );
        assert!(p.recommendation.contains("TermsCmd -> crates/x/src/terms.rs"));
        assert!(p.manifest_sketch.contains("crates/x/src/terms.rs"));
    }

    #[test]
    fn shared_stems_cluster() {
        let src = "pub fn store_load() {}\npub fn store_save() {}\npub fn other() {}\n";
        let p = plan("src/a.rs", src);
        assert!(p.recommendation.starts_with("prefix-cluster"), "{}", p.recommendation);
        assert!(p.recommendation.contains("store <- store_load, store_save"));
    }

    #[test]
    fn strings_and_comments_never_corrupt_depth() {
        let src = "pub fn a() {\n    let s = \"}\"; // }\n}\n\npub fn b() {}\n";
        let items = item_spans(src);
        assert_eq!(items.len(), 2, "{items:#?}");
        assert_eq!(items[1].name, "b");
    }

    #[test]
    fn the_dispatch_itself_is_never_a_split_unit() {
        // Regression from dogfooding on harna-cli/main.rs: bare
        // `Command` matched the *Command filter and sketched `.rs`.
        let src = "pub enum Command {\n    A,\n}\n\npub enum ThingCmd {\n    B,\n}\n\nfn run() {\n";
        let src = src.to_string() + &"    x();\n".repeat(30) + "}\n";
        let p = plan("src/main.rs", &src);
        assert!(p.recommendation.contains("ThingCmd -> src/thing.rs"), "{}", p.recommendation);
        assert!(!p.recommendation.contains("Command ->"), "{}", p.recommendation);
        assert!(!p.manifest_sketch.contains("src/.rs"), "{}", p.manifest_sketch);
    }

    #[test]
    fn impl_names_read_clean() {
        let items = item_spans("pub struct S {}\n\nimpl S {\n}\n");
        assert_eq!(items[1].name, "S", "{items:#?}");
        assert_eq!(items[1].kind, "impl");
    }
}
