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

use super::parity::{ParityEvent, ParityTarget};

/// The status of a target that is where the change was made.
pub const ORIGIN: &str = "origin";

/// A change reason that can break importers.
pub fn is_breaking(reason: &str) -> bool {
    reason.starts_with("module removed") || reason.starts_with("signature removed")
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
        let kind = if changed_here.iter().any(|r| is_breaking(r)) { "breaking" } else { "additive" };
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
        if matches!(prev.status.as_str(), "ack" | "noop") && prev.breaking && prev.reason == breaking_reason(upstream_breaking) {
            return Placement {
                status: if prev.status == "ack" { "ack" } else { "noop" },
                note: format!("{} (carried from an earlier generation: same breakage)", prev.note),
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
    format!("imports a module with a breaking change: {}", upstream_breaking.join("; "))
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
        if self.hand_resolved == 0 { 0 } else { self.signals() * 100 / self.hand_resolved }
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
        let hand: Vec<&ParityTarget> =
            ev.targets.iter().filter(|t| matches!(t.status.as_str(), "ack" | "noop")).collect();
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
        }
    }

    #[test]
    fn origins_and_additive_importers_need_no_ack() {
        let o = place(&["signature added: pub fn f()".into()], &[], None, 1);
        assert_eq!((o.status, o.breaking), (ORIGIN, false));
        assert!(o.note.contains("additive"));
        let b = place(&["signature removed: pub fn g()".into()], &[], None, 1);
        assert_eq!(b.status, ORIGIN, "even a breaking origin: its author made the change");
        assert!(b.note.contains("breaking"));
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
        let grown = vec![up[0].clone(), "crates/x/a.rs: signature removed: pub fn h()".into()];
        assert_eq!(place(&[], &grown, Some(&prev), 2).status, "open");
    }

    #[test]
    fn a_split_into_the_module_family_is_a_move_not_a_breakage() {
        use crate::search::digest::{Digest, Module};
        let m = |path: &str, sigs: &[&str]| Module {
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
        assert_eq!(moved_within_family("src/net/mod.rs", "pub fn get()", &fresh).as_deref(), Some("src/net/client.rs"));
        // Into an unrelated module: importers' paths may break — still breaking.
        assert_eq!(moved_within_family("src/store.rs", "pub fn moved_far()", &fresh), None);
        assert_eq!(moved_within_family("src/store.rs", "pub fn gone()", &fresh), None);
        assert!(!is_breaking("signature moved to src/store/registers.rs: pub fn load_roadmap(&self)"));
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
        assert_eq!(r, StampReport { hand_resolved: 4, blank: 1, repeated: 2 });
        assert_eq!(r.rate(), 75);
        assert!(r.render().contains("rubber-stamp signals 3 (75%)"));
        assert!(stamp_report(&[]).render().contains("hand-resolved: none"));
    }
}
