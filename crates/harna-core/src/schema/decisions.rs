//! Component `decision-log`: the decision record set as data.
//!
//! Decisions are ADR-style markdown files in `.harna/decisions/`; this
//! component understands their one structural field — `supersedes` — so
//! superseded decisions stay referenced and visibly dead instead of
//! silently contradicting the current plan. Nothing is patched in place:
//! a new decision supersedes an old one, and the log renders the chain.

use std::fs;

use crate::schema::store::Store;

/// One recorded decision.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// Filename within the decisions directory.
    pub file: String,
    /// First-level heading text.
    pub title: String,
    /// The decision file this one supersedes, if any.
    pub supersedes: Option<String>,
}

/// The file body for a decision record: title, optional supersedes pointer,
/// then the free-form body.
pub fn markdown(title: &str, body: &str, supersedes: Option<&str>) -> String {
    match supersedes {
        Some(s) => format!("# {title}\n\nsupersedes: {s}\n\n{body}\n"),
        None => format!("# {title}\n\n{body}\n"),
    }
}

/// Parse one decision file's contents.
pub fn parse(file: &str, raw: &str) -> Decision {
    let title = raw
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .unwrap_or_default()
        .trim()
        .to_string();
    let supersedes = raw.lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("supersedes:")
            .map(|v| v.trim().replace('\\', "/"))
    });
    Decision { file: file.into(), title, supersedes }
}

/// All decisions, sorted by filename (date ordering falls out of the slug).
pub fn list(store: &Store) -> anyhow::Result<Vec<Decision>> {
    let dir = store.decisions_dir();
    let mut decisions = Vec::new();
    if dir.is_dir() {
        let mut files: Vec<_> = fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
            .collect();
        files.sort();
        for p in files {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let raw = fs::read_to_string(&p)?;
            decisions.push(parse(&name, &raw));
        }
    }
    Ok(decisions)
}

/// Files that have been superseded by at least one other decision.
fn superseded(decisions: &[Decision]) -> Vec<&str> {
    decisions
        .iter()
        .filter_map(|d| d.supersedes.as_deref())
        .collect()
}

/// Is `file` visibly dead — a later decision supersedes it?
pub fn is_superseded(file: &str, decisions: &[Decision]) -> bool {
    superseded(decisions).contains(&file)
}

/// Token-efficient listing: every decision, dead ones visibly dead with the
/// pointer to their successor.
pub fn render(decisions: &[Decision]) -> String {
    if decisions.is_empty() {
        return "no decisions recorded".into();
    }
    let mut out = String::new();
    for d in decisions {
        if let Some(s) = &d.supersedes {
            out.push_str(&format!("* {} — supersedes {}\n", d.file, s));
        } else if is_superseded(&d.file, decisions) {
            let successors: Vec<&str> = decisions
                .iter()
                .filter(|x| x.supersedes.as_deref() == Some(d.file.as_str()))
                .map(|x| x.file.as_str())
                .collect();
            out.push_str(&format!(
                "* {} — superseded by {}\n",
                d.file,
                successors.join(", ")
            ));
        } else {
            out.push_str(&format!("* {}\n", d.file));
        }
        out.push_str(&format!("    {}\n", d.title));
    }
    out
}

/// The chain around one decision: its successors (decisions superseding
/// it), itself, then its supersedes ancestors. Unknown or ambiguous slugs
/// are reported.
pub fn chain(decisions: &[Decision], query: &str) -> anyhow::Result<String> {
    let matches = matching(decisions, query);
    match matches.len() {
        0 => anyhow::bail!(
            "no decision matches `{query}` — see `harna decisions` for the list"
        ),
        _ if matches.len() > 1 => anyhow::bail!(
            "`{query}` is ambiguous: {}",
            matches.iter().map(|d| d.file.as_str()).collect::<Vec<_>>().join(", ")
        ),
        _ => {}
    }
    let d = matches.get(0).unwrap();
    let mut lines: Vec<String> = Vec::new();
    for s in decisions
        .iter()
        .filter(|x| x.supersedes.as_deref() == Some(d.file.as_str()))
    {
        lines.push(format!("* {} — {}  (superseded by {})", s.file, s.title, d.file));
    }
    lines.push(format!("* {} — {}", d.file, d.title));
    let mut cursor = d.supersedes.clone();
    while let Some(anc_file) = cursor {
        match decisions.iter().find(|x| x.file == anc_file) {
            Some(a) => {
                lines.push(format!("* {} — {}", a.file, a.title));
                cursor = a.supersedes.clone();
            }
            None => {
                lines.push(format!("  ! supersedes points at unknown `{anc_file}`"));
                break;
            }
        }
    }
    Ok(lines.join("\n"))
}

fn matching<'a>(decisions: &'a [Decision], query: &str) -> Vec<&'a Decision> {
    let q = query.replace('\\', "/");
    let q = q.trim_end_matches(".md");
    if q.is_empty() {
        return decisions.iter().collect();
    }
    decisions
        .iter()
        .filter(|d| d.file.starts_with(q))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(file: &str, title: &str, supersedes: Option<&str>) -> Decision {
        Decision { file: file.into(), title: title.into(), supersedes: supersedes.map(|s| s.into()) }
    }

    #[test]
    fn markdown_carries_the_pointer() {
        let plain = markdown("T", "body", None);
        assert_eq!(plain, "# T\n\nbody\n");
        assert_eq!(parse("x.md", &plain), d("x.md", "T", None));
        let chained = markdown("T", "body", Some("old.md"));
        let parsed = parse("new.md", &chained);
        assert_eq!(parsed.supersedes.as_deref(), Some("old.md"));
        assert_eq!(parsed.title, "T");
    }

    #[test]
    fn parse_tolerates_variants() {
        // indented pointer, CRLF, no title.
        let raw = "# Title\r\n\r\n   supersedes:  old.md\r\n\r\nbody\r\n";
        assert_eq!(parse("n.md", raw).supersedes.as_deref(), Some("old.md"));
        let bare = "prose without a heading or pointer";
        let p = parse("b.md", bare);
        assert_eq!(p.title, "");
        assert!(p.supersedes.is_none());
    }

    #[test]
    fn render_marks_visibly_dead() {
        let set = vec![d("a.md", "Old plan", None), d("b.md", "New plan", Some("a.md"))];
        assert!(is_superseded("a.md", &set));
        assert!(!is_superseded("b.md", &set));
        let r = render(&set);
        assert!(r.contains("a.md"), "stays referenced");
        assert!(r.contains("supersedes a.md"));
        assert!(render(&[]).contains("no decisions"));
    }

    #[test]
    fn chain_walks_both_directions() {
        let set = vec![
            d("1.md", "first", None),
            d("2.md", "second", Some("1.md")),
            d("3.md", "third", Some("2.md")),
        ];
        let c = chain(&set, "2").unwrap();
        assert!(c.contains("1.md"), "ancestor listed");
        assert!(c.contains("2.md"));
        assert!(c.contains("3.md"), "successor listed");
        assert!(chain(&set, "ghost.md").is_err());
        assert!(chain(&set, "").is_err(), "prefix match must not be ambiguous");
    }

    #[test]
    fn list_reads_the_real_store() {
        let tmp = std::env::temp_dir().join(format!("harna-dec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let old = markdown("Old", "done", None);
        fs::write(
            store.decisions_dir().join("2026-01-01-old.md"),
            old,
        )
        .unwrap();
        fs::write(
            store.decisions_dir().join("2026-01-02-new.md"),
            markdown("New", "replaces it", Some("2026-01-01-old.md")),
        )
        .unwrap();

        let decisions = list(&store).unwrap();
        assert_eq!(decisions.len(), 2);
        assert!(is_superseded("2026-01-01-old.md", &decisions));
        assert!(render(&decisions).contains("2026-01-02-new.md"));
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    }
}
