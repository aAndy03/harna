//! Component `parity-policy` (0.3.35): which parity targets deserve a
//! human, and how often the humans rubber-stamp.
//!
//! A parity event fans a structural diff out to the changed modules and
//! their importers. Asking someone to ack every one of them trains them
//! to ack without reading — the ledger then certifies ceremony, not
//! review. The policy keeps an ack only where it carries information:
//! - the **origin** (a module the change was made in) resolves as
//!   `origin`: its author's commit is the record, an ack adds nothing;
//! - an **importer of an additive change** (only added modules,
//!   signatures or deps upstream) resolves `auto`: nothing it uses went
//!   away;
//! - an **importer of a breaking change** (a removed signature or module
//!   upstream) stays `open` and is marked `breaking`, so the `auto` pass
//!   never waves it through just because its own surface is unchanged —
//!   that is exactly the importer that may now be calling something gone.
//!
//! `stamp_report` measures what remains: hand resolutions with a blank
//! note or a note copied verbatim across targets.

use std::collections::HashMap;

use super::{ParityEvent, ParityTarget};

/// The status of a target that is where the change was made.
pub const ORIGIN: &str = "origin";

/// A change reason that can break importers.
pub fn is_breaking(reason: &str) -> bool {
    reason.starts_with("module removed") || reason.starts_with("signature removed")
}

/// Whether a breaking `reason` on `changed` can reach `importer` (0.3.47):
/// a removed `pub(crate)` item breaks only its own crate, a removed
/// `pub(super)` item only the parent module's subtree. A crate that
/// imports the module for its `pub` surface never saw them.
pub fn breaks(
    reason: &str,
    changed: &crate::search::digest::Module,
    importer: &crate::search::digest::Module,
) -> bool {
    let Some(sig) = reason.strip_prefix("signature removed: ") else {
        return true;
    };
    let (cdir, cstem) = changed.crate_of();
    let (idir, istem) = importer.crate_of();
    if sig.starts_with("pub(crate)") || sig.starts_with("pub(in ") {
        return cdir == idir;
    }
    if sig.starts_with("pub(super)") {
        let module = cstem.strip_suffix("/mod").unwrap_or(&cstem);
        let parent = module.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        return cdir == idir
            && (parent.is_empty()
                || changed.is_crate_root()
                || istem == parent
                || istem.starts_with(&format!("{parent}/")));
    }
    true
}

/// Symbol-level reach (0.7.3, deferred `symbol-level-parity`): an
/// importer is touched by a removed signature only if its source names the
/// symbol — a function, type or method name, word-bounded. A call through
/// a struct field (`self.store.save(`) still names `save`, so it is
/// caught; an importer of other items of the module is not. A reason
/// whose symbol cannot be read keeps the module-level answer (true).
pub fn names_symbol(importer_src: &str, reason: &str) -> bool {
    let Some(sig) = reason.strip_prefix("signature removed: ") else {
        return true;
    };
    if importer_src.is_empty() {
        return true; // unreadable here: the module-level answer stands
    }
    // the word after a declaring keyword that is not itself a keyword
    // (0.7.7: `const fn total` read as the symbol `fn`)
    const DECL: &[&str] = &[
        "fn",
        "struct",
        "enum",
        "trait",
        "type",
        "const",
        "static",
        "union",
        "mod",
        "function",
        "class",
        "interface",
    ];
    const QUALIFIERS: &[&str] = &["async", "unsafe", "extern", "pub", "mut"];
    let words: Vec<&str> = sig
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .filter(|w| !w.is_empty())
        .collect();
    let Some(name) = words.windows(2).find_map(|w| {
        (DECL.contains(&w[0]) && !DECL.contains(&w[1]) && !QUALIFIERS.contains(&w[1]))
            .then(|| w[1].to_string())
    }) else {
        return true;
    };
    // word-bounded for `$` names too; the whole source minus comments —
    // a call inside a test or a macro body still names it (0.7.7)
    let word = regex::Regex::new(&format!(
        r"(?:^|[^\w$]){}(?:[^\w$]|$)",
        regex::escape(&name)
    ));
    word.map_or(true, |w| {
        without_comments(importer_src)
            .lines()
            .any(|l| w.is_match(l))
    })
}

/// The source with `//` and `/* */` comments blanked; string literals
/// (`"…"`, `\`…\``) kept whole so a `//` inside a URL is not a comment.
fn without_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('/', Some('/')) => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                    }
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                out.push(' ');
            }
            ('"' | '`', _) => {
                quote = Some(c);
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Where a "removed" signature actually went, when it moved into its own
/// module family: `store.rs` split into `store/registers.rs` keeps the
/// identical signature on the same type (or re-exported), so importers
/// are untouched — a split is not a breakage (0.3.37 dogfood: 44 false
/// "breaking" importers). A move to an unrelated module stays breaking.
pub fn moved_within_family(
    path: &str,
    sig: &str,
    fresh: &crate::search::digest::Digest,
) -> Option<String> {
    let stem = path.strip_suffix(".rs")?;
    // `x/mod.rs` owns the folder `x/`; `x.rs` owns `x/` as well.
    let family = stem.strip_suffix("/mod").unwrap_or(stem);
    fresh
        .modules
        .iter()
        .filter(|m| m.path != path && m.path.starts_with(&format!("{family}/")))
        .find(|m| m.signatures.iter().any(|s| s == sig))
        .map(|m| m.path.clone())
}

/// The first-sight resolution for a target in a (re)opened event.
pub struct Placement {
    pub status: &'static str,
    pub note: String,
    pub breaking: bool,
}

/// Decide a target's status. `changed_here` are this module's own change
/// reasons (non-empty = origin); `upstream_breaking` are the breaking
/// reasons of the modules it imports; `prior` is the previous
/// generation's target, if any.
pub fn place(
    changed_here: &[String],
    upstream_breaking: &[String],
    prior: Option<&ParityTarget>,
    generation: u32,
) -> Placement {
    if !changed_here.is_empty() {
        let kind = if changed_here.iter().any(|r| is_breaking(r)) {
            "API removed or changed here; importers are reviewed"
        } else {
            "additive"
        };
        return Placement {
            status: ORIGIN,
            note: format!("origin of the change ({kind}) — the commit is the record"),
            breaking: false,
        };
    }
    if upstream_breaking.is_empty() {
        return Placement {
            status: "auto",
            note: "additive upstream change — nothing it imports was removed".into(),
            breaking: false,
        };
    }
    // A breaking importer already reviewed for this exact breakage keeps
    // its verdict; a new or grown breakage needs a fresh look.
    if let Some(prev) = prior {
        if matches!(prev.status.as_str(), "ack" | "noop")
            && prev.breaking
            && prev.reason == breaking_reason(upstream_breaking)
        {
            return Placement {
                status: if prev.status == "ack" { "ack" } else { "noop" },
                note: format!(
                    "{} (carried from an earlier generation: same breakage)",
                    prev.note
                ),
                breaking: true,
            };
        }
    }
    Placement {
        status: "open",
        note: if prior.is_some() {
            format!("re-opened at gen {generation}: upstream breakage changed")
        } else {
            String::new()
        },
        breaking: true,
    }
}

/// The reason line for a breaking importer: what it may now be missing.
pub fn breaking_reason(upstream_breaking: &[String]) -> String {
    format!(
        "imports a module with a breaking change: {}",
        upstream_breaking.join("; ")
    )
}

/// How much of the hand-resolved ledger looks like rubber stamps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StampReport {
    /// Targets resolved by a person or agent (ack | noop).
    pub hand_resolved: usize,
    /// …with no note at all.
    pub blank: usize,
    /// …whose note is copied verbatim onto another target of the event.
    pub repeated: usize,
}

impl StampReport {
    pub fn signals(&self) -> usize {
        self.blank + self.repeated
    }

    /// Percentage of hand resolutions that look stamped (0 when none).
    pub fn rate(&self) -> usize {
        (self.signals() * 100)
            .checked_div(self.hand_resolved)
            .unwrap_or(0)
    }

    pub fn render(&self) -> String {
        if self.hand_resolved == 0 {
            return "hand-resolved: none (every target was origin, auto or still open)\n".into();
        }
        format!(
            "hand-resolved: {} · rubber-stamp signals {} ({}%): {} blank, {} copied notes\n",
            self.hand_resolved,
            self.signals(),
            self.rate(),
            self.blank,
            self.repeated
        )
    }
}

pub fn stamp_report(events: &[ParityEvent]) -> StampReport {
    let mut r = StampReport::default();
    for ev in events {
        let hand: Vec<&ParityTarget> = ev
            .targets
            .iter()
            .filter(|t| matches!(t.status.as_str(), "ack" | "noop"))
            .collect();
        let mut uses: HashMap<&str, usize> = HashMap::new();
        for t in &hand {
            let n = t.note.trim();
            if !n.is_empty() {
                *uses.entry(n).or_default() += 1;
            }
        }
        for t in &hand {
            r.hand_resolved += 1;
            let n = t.note.trim();
            if n.is_empty() {
                r.blank += 1;
            } else if uses.get(n).copied().unwrap_or(0) > 1 {
                r.repeated += 1;
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(status: &str, note: &str) -> ParityTarget {
        ParityTarget {
            path: format!("m{}.rs", note.len()),
            reason: String::new(),
            status: status.into(),
            note: note.into(),
            by: String::new(),
            when: String::new(),
            breaking: false,
            history: Vec::new(),
        }
    }

    /// 0.3.47: a removed `pub(crate)` item cannot break another crate,
    /// and a removed `pub(super)` one only its parent module's subtree.
    #[test]
    fn removals_break_only_where_the_item_was_visible() {
        let m = |path: &str, dir: &str| {
            let mut m = super::super::fixtures::module(path, &[], &[]);
            m.crate_dir = Some(dir.into());
            m.src_root = Some(format!("{dir}/src"));
            m
        };
        let changed = m("crates/x/src/schema/store.rs", "crates/x");
        let sibling = m("crates/x/src/schema/view.rs", "crates/x");
        let elsewhere = m("crates/x/src/cli/run.rs", "crates/x");
        let other_crate = m("crates/y/src/api.rs", "crates/y");
        let crate_wide = "signature removed: pub(crate) fn load()";
        assert!(breaks(crate_wide, &changed, &elsewhere));
        assert!(!breaks(crate_wide, &changed, &other_crate));
        let parent_only = "signature removed: pub(super) fn load()";
        assert!(breaks(parent_only, &changed, &sibling));
        assert!(!breaks(parent_only, &changed, &elsewhere));
        assert!(breaks(
            "signature removed: pub fn load()",
            &changed,
            &other_crate
        ));
        assert!(breaks("module removed", &changed, &other_crate));
    }

    #[test]
    fn origins_and_additive_importers_need_no_ack() {
        let o = place(&["signature added: pub fn f()".into()], &[], None, 1);
        assert_eq!((o.status, o.breaking), (ORIGIN, false));
        assert!(o.note.contains("additive"));
        let b = place(&["signature removed: pub fn g()".into()], &[], None, 1);
        assert_eq!(
            b.status, ORIGIN,
            "even a breaking origin: its author made the change"
        );
        assert!(b.note.contains("API removed or changed here"));
        let a = place(&[], &[], None, 1);
        assert_eq!(a.status, "auto");
    }

    #[test]
    fn breaking_importers_stay_open_until_reviewed_for_that_breakage() {
        let up = vec!["crates/x/a.rs: signature removed: pub fn g()".to_string()];
        let first = place(&[], &up, None, 1);
        assert_eq!((first.status, first.breaking), ("open", true));
        // Reviewed in gen 1 for exactly this breakage: the verdict carries.
        let mut prev = target("ack", "updated the call site");
        prev.breaking = true;
        prev.reason = breaking_reason(&up);
        let carried = place(&[], &up, Some(&prev), 2);
        assert_eq!(carried.status, "ack");
        // The breakage grew: look again.
        let grown = vec![
            up[0].clone(),
            "crates/x/a.rs: signature removed: pub fn h()".into(),
        ];
        assert_eq!(place(&[], &grown, Some(&prev), 2).status, "open");
    }

    #[test]
    fn a_split_into_the_module_family_is_a_move_not_a_breakage() {
        use crate::search::digest::{Digest, Module};
        let m = |path: &str, sigs: &[&str]| Module {
            reexports: Vec::new(),
            crate_dir: None,
            src_root: None,
            path: path.into(),
            lang: "rust".into(),
            signatures: sigs.iter().map(|s| s.to_string()).collect(),
            deps: vec![],
            body_hash: String::new(),
            inventory: vec![],
            mods: vec![],
        };
        let fresh = Digest {
            modules: vec![
                m("src/store.rs", &[]),
                m("src/store/registers.rs", &["pub fn load_roadmap(&self)"]),
                m("src/other.rs", &["pub fn moved_far()"]),
                m("src/net/mod.rs", &[]),
                m("src/net/client.rs", &["pub fn get()"]),
            ],
            ..Default::default()
        };
        assert_eq!(
            moved_within_family("src/store.rs", "pub fn load_roadmap(&self)", &fresh).as_deref(),
            Some("src/store/registers.rs")
        );
        assert_eq!(
            moved_within_family("src/net/mod.rs", "pub fn get()", &fresh).as_deref(),
            Some("src/net/client.rs")
        );
        // Into an unrelated module: importers' paths may break — still breaking.
        assert_eq!(
            moved_within_family("src/store.rs", "pub fn moved_far()", &fresh),
            None
        );
        assert_eq!(
            moved_within_family("src/store.rs", "pub fn gone()", &fresh),
            None
        );
        assert!(!is_breaking(
            "signature moved to src/store/registers.rs: pub fn load_roadmap(&self)"
        ));
    }

    #[test]
    fn stamp_report_counts_blank_and_copied_notes() {
        let ev = ParityEvent {
            id: "e".into(),
            kind: "digest-diff".into(),
            source: String::new(),
            generation: 1,
            baseline_ref: "HEAD".into(),
            created: String::new(),
            updated: String::new(),
            targets: vec![
                target("ack", "updated callers in api"),
                target("ack", ""),
                target("noop", "done"),
                {
                    let mut t = target("noop", "done");
                    t.path = "other.rs".into();
                    t
                },
                target("auto", "verified"),
                target(ORIGIN, "origin"),
            ],
        };
        let r = stamp_report(&[ev]);
        assert_eq!(
            r,
            StampReport {
                hand_resolved: 4,
                blank: 1,
                repeated: 2
            }
        );
        assert_eq!(r.rate(), 75);
        assert!(r.render().contains("rubber-stamp signals 3 (75%)"));
        assert!(stamp_report(&[]).render().contains("hand-resolved: none"));
    }
}

#[cfg(test)]
mod symbol_tests {
    use super::names_symbol;

    /// 0.7.3 (household F7): an importer is touched by a removed signature
    /// only when it names the symbol — through a struct field too; one
    /// importing other items of the module is not; comments do not count.
    #[test]
    fn importers_owe_only_for_symbols_they_name() {
        let removed = "signature removed: pub fn save(&self, e: Entry) -> Result<()>";
        assert!(names_symbol(
            "fn post(&self) { self.store.save(e)?; }",
            removed
        ));
        assert!(!names_symbol(
            "use crate::store::Store;\nfn f(s: &Store) { s.load(); }",
            removed
        ));
        assert!(!names_symbol("// save is gone\nfn f() {}", removed));
        assert!(names_symbol("", removed), "unreadable: module-level answer");
        // 0.7.7: `const fn` names the function; `$` bounds a TS name;
        // a call in a test module still counts; `/* */` does not
        let konst = "signature removed: pub const fn total() -> u8";
        assert!(names_symbol(
            "#[cfg(test)]\nmod t { fn x() { total(); } }",
            konst
        ));
        assert!(!names_symbol("fn f() { /* total */ }", konst));
        let ts = "signature removed: export function get(id: string)";
        assert!(!names_symbol("const $get = 1;", ts));
        assert!(names_symbol("const u = \"http://x\"; get(1);", ts));
        assert!(
            names_symbol("fn f() {}", "signature changed: x"),
            "other reasons stand"
        );
    }
}
