//! Component `assumptions-data` (1.1.2): what the agent assumed, and what
//! it must ask.
//!
//! A prompt is a vision, not a spec: 300 words cannot settle every
//! question a codebase meets on its way there. An agent that guesses
//! silently builds the guess into the code, and the human finds it later.
//! The register makes each guess data the human can review: what was
//! assumed (with the default the code follows meanwhile), the question that
//! would settle it, what it affects, and the milestone it must be settled
//! by. `harna assume next` hands the human the next small batch; a blocking
//! assumption holds the release until it is answered.

use serde::{Deserialize, Serialize};

pub mod vision;

fn current() -> u32 {
    crate::SCHEMA_BASELINE
}

/// `.harna/assumptions.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Assumptions {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<Assumption>,
}

/// One assumption.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Assumption {
    /// Stable id, e.g. "accounts-are-personal".
    pub id: String,
    /// What was assumed, as a statement the code now follows.
    pub statement: String,
    /// The question that settles it, put to the human.
    pub question: String,
    /// What the code does until it is settled, if not the statement itself.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default: String,
    /// What the answer would change (data model, a screen, a service).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub impact: String,
    /// The roadmap milestone it must be settled by, if any.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    /// A blocking assumption holds the release until it is answered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blocking: bool,
    /// open | confirmed | corrected (absent is open).
    #[serde(default = "open", skip_serializing_if = "is_open")]
    pub state: String,
    /// The human's answer, once settled.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub answer: String,
    /// When it was recorded (ISO date).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recorded: String,
}

fn open() -> String {
    "open".into()
}

fn is_open(s: &String) -> bool {
    s == "open"
}

/// The states an assumption may be in.
pub const STATES: &[&str] = &["open", "confirmed", "corrected"];

impl Assumption {
    /// Fails closed: anything not settled as confirmed or corrected is
    /// still open (review of 1.1.2: a hand-typed `state = "confimed"`
    /// slipped past the release gate).
    pub fn is_open(&self) -> bool {
        !matches!(self.state.as_str(), "confirmed" | "corrected")
    }

    /// One line for a list: id, state, the statement, and what settles it.
    pub fn line(&self) -> String {
        let mut out = format!("[{}] {}", self.id, self.statement);
        if self.is_open() {
            out.push_str(&format!(" — ask: {}", self.question));
            if !self.default.is_empty() {
                out.push_str(&format!(" (meanwhile: {})", self.default));
            }
        } else {
            out.push_str(&format!(" — {}", self.state));
            if !self.answer.is_empty() {
                out.push_str(&format!(": {}", self.answer));
            }
        }
        let mut tags = Vec::new();
        if self.blocking && self.is_open() {
            tags.push("blocking".to_string());
        }
        if !self.by.is_empty() {
            tags.push(format!("by {}", self.by));
        }
        if !self.impact.is_empty() {
            tags.push(format!("affects {}", self.impact));
        }
        if !tags.is_empty() {
            out.push_str(&format!(" [{}]", tags.join("; ")));
        }
        out
    }
}

/// The intent in briefing lines (onboarding, status, finish): the vision
/// kept, and the open assumptions with the command that asks them. Empty
/// when there is neither.
pub fn brief(vision: Option<&str>, reg: &Assumptions) -> String {
    let mut out = String::new();
    if let Some(line) = vision.and_then(vision::summary) {
        out.push_str(&format!(
            "vision: .harna/vision.md ({line}) — build toward it in stages; the roadmap holds the stages\n"
        ));
    }
    let open = reg.open();
    if !open.is_empty() {
        let blocking = open.iter().filter(|a| a.blocking).count();
        out.push_str(&format!(
            "assumptions: {} open{} — `harna assume next` hands the owner the questions; record each new guess with `harna assume add`\n",
            open.len(),
            if blocking > 0 {
                format!(" ({blocking} blocking the release)")
            } else {
                String::new()
            }
        ));
    }
    out
}

/// The questions for the human, numbered, as `assume next` prints them.
pub fn ask(next: &[&Assumption]) -> String {
    if next.is_empty() {
        return "no open assumptions — nothing to ask\n".into();
    }
    let mut out = String::from(
        "questions for the owner (answer with `harna assume confirm|correct <id>`):\n",
    );
    for (i, a) in next.iter().enumerate() {
        out.push_str(&format!(
            "  {}. [{}] {}{}\n     assumed: {}{}\n",
            i + 1,
            a.id,
            a.question,
            if a.blocking {
                " (blocks the release)"
            } else {
                ""
            },
            a.statement,
            if a.default.is_empty() {
                String::new()
            } else {
                format!(" — meanwhile the code does: {}", a.default)
            }
        ));
    }
    out
}

impl Assumptions {
    pub fn is_empty(&self) -> bool {
        self.assumptions.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&Assumption> {
        self.assumptions
            .iter()
            .find(|a| a.id.eq_ignore_ascii_case(id))
    }

    /// Add or replace by id.
    /// Record one; the same id replaces an open one. A settled one is the
    /// owner's answer: replacing it needs `replace` (review of 1.1.2: a
    /// re-run `assume add` silently reopened a confirmed question).
    pub fn upsert(&mut self, a: Assumption, replace: bool) -> anyhow::Result<()> {
        if let Some(old) = self
            .assumptions
            .iter()
            .find(|x| x.id.eq_ignore_ascii_case(&a.id) && !x.is_open())
        {
            if !replace {
                anyhow::bail!(
                    "assumption `{}` is already {} by the owner — record a new id, or replace it on purpose (--replace)",
                    old.id,
                    old.state
                );
            }
        }
        self.assumptions
            .retain(|x| !x.id.eq_ignore_ascii_case(&a.id));
        self.assumptions.push(a);
        self.assumptions.sort_by(|x, y| x.id.cmp(&y.id));
        Ok(())
    }

    /// The open ones that hold the release: blocking, or due — settled
    /// `by` a milestone that is now underway or already done (review of
    /// 1.1.2: the gate read `blocking` alone and `by` was decoration).
    pub fn holding(
        &self,
        roadmap: &crate::schema::registers::roadmap::Roadmap,
    ) -> Vec<&Assumption> {
        self.open()
            .into_iter()
            .filter(|a| {
                a.blocking
                    || (!a.by.is_empty()
                        && roadmap
                            .find(&a.by)
                            .is_some_and(|m| matches!(m.state.as_str(), "current" | "done")))
            })
            .collect()
    }

    /// Every assumption for a list: open first, blocking first, then by id.
    pub fn listed(&self) -> Vec<&Assumption> {
        let mut all: Vec<&Assumption> = self.assumptions.iter().collect();
        all.sort_by_key(|a| (!a.is_open(), !a.blocking, a.id.clone()));
        all
    }

    /// Settle one: `confirmed` keeps the statement, `corrected` records
    /// the answer the code must now follow. Errs on an unknown id or state.
    pub fn settle(&mut self, id: &str, state: &str, answer: &str) -> anyhow::Result<&Assumption> {
        if !matches!(state, "confirmed" | "corrected") {
            anyhow::bail!("an assumption is settled as confirmed or corrected, not `{state}`");
        }
        if state == "corrected" && answer.trim().is_empty() {
            anyhow::bail!("a correction says what is true instead (--answer \"..\")");
        }
        let a = self
            .assumptions
            .iter_mut()
            .find(|a| a.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| anyhow::anyhow!("no assumption `{id}` (harna assume list)"))?;
        a.state = state.into();
        a.answer = answer.trim().to_string();
        Ok(a)
    }

    pub fn open(&self) -> Vec<&Assumption> {
        self.assumptions.iter().filter(|a| a.is_open()).collect()
    }

    /// The next `n` questions for the human: blocking first, then the
    /// earliest milestone (in roadmap order; none last), then the oldest.
    pub fn next(&self, n: usize, milestone_order: &[String]) -> Vec<&Assumption> {
        let rank = |a: &Assumption| {
            let at = milestone_order
                .iter()
                .position(|m| m == &a.by)
                .unwrap_or(usize::MAX);
            (!a.blocking, at, a.recorded.clone(), a.id.clone())
        };
        let mut open = self.open();
        open.sort_by_key(|a| rank(a));
        open.into_iter().take(n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: &str, by: &str, blocking: bool, recorded: &str) -> Assumption {
        Assumption {
            id: id.into(),
            statement: "s".into(),
            question: "q?".into(),
            by: by.into(),
            blocking,
            state: "open".into(),
            recorded: recorded.into(),
            ..Default::default()
        }
    }

    #[test]
    fn the_next_questions_are_blocking_then_nearest_then_oldest() {
        let mut r = Assumptions::default();
        r.upsert(a("late", "1.2", false, "2026-01-01"), false)
            .unwrap();
        r.upsert(a("soon", "1.1", false, "2026-03-01"), false)
            .unwrap();
        r.upsert(a("block", "1.2", true, "2026-05-01"), false)
            .unwrap();
        r.upsert(a("loose", "", false, "2025-01-01"), false)
            .unwrap();
        let order = vec!["1.1".to_string(), "1.2".to_string()];
        let ids: Vec<&str> = r.next(3, &order).iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["block", "soon", "late"]);
        r.settle("block", "confirmed", "").unwrap();
        assert_eq!(r.next(1, &order)[0].id, "soon");
        assert!(
            r.settle("soon", "corrected", "").is_err(),
            "a correction says what is true"
        );
        assert!(r.settle("nope", "confirmed", "").is_err());
    }

    #[test]
    fn an_open_assumption_is_written_without_its_state() {
        let mut r = Assumptions::default();
        r.upsert(a("x", "", false, ""), false).unwrap();
        let raw = toml::to_string_pretty(&r).unwrap();
        assert!(!raw.contains("state ="), "{raw}");
        let back: Assumptions = toml::from_str(&raw).unwrap();
        assert!(back.assumptions[0].is_open());
    }

    #[test]
    fn an_unknown_state_is_read_as_open() {
        let mut x = a("q", "", true, "2026-10-01");
        x.state = "confimed".into();
        assert!(x.is_open(), "a typo must not settle it");
        x.state = "confirmed".into();
        assert!(!x.is_open());
    }

    #[test]
    fn a_settled_answer_is_not_reopened_by_accident() {
        let mut r = Assumptions::default();
        r.upsert(a("x", "", false, ""), false).unwrap();
        r.upsert(a("x", "", true, ""), false).unwrap(); // open: replaced
        r.settle("x", "confirmed", "yes").unwrap();
        assert!(r.upsert(a("x", "", false, ""), false).is_err());
        r.upsert(a("x", "", false, ""), true).unwrap();
        assert!(r.assumptions[0].is_open(), "replaced on purpose");
    }

    #[test]
    fn an_assumption_due_now_holds_the_release_like_a_blocking_one() {
        use crate::schema::registers::roadmap::{Milestone, Roadmap};
        let mut road = Roadmap::default();
        for (id, state) in [("1.1", "done"), ("1.2", "current"), ("1.3", "declared")] {
            road.upsert(Milestone {
                id: id.into(),
                title: "t".into(),
                state: state.into(),
                notes: String::new(),
            });
        }
        let mut r = Assumptions::default();
        for (id, by, blocking) in [
            ("past", "1.1", false),
            ("now", "1.2", false),
            ("later", "1.3", false),
            ("held", "", true),
            ("free", "", false),
        ] {
            r.upsert(a(id, by, blocking, ""), false).unwrap();
        }
        let ids: Vec<&str> = r.holding(&road).iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["held", "now", "past"]);
    }
}
