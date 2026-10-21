//! Component `approvals-data` (1.1.4): what the owner approved, and which
//! changes need the owner's yes first.
//!
//! An agent changes an approved look one prompt at a time; a dependency,
//! a migration or a payment path lands because nobody was asked. The
//! register makes both data:
//! - an **approval** freezes a set of paths by content hash, with who
//!   approved it, when and at which version (`calendar@3`). A later change
//!   to those files is a debt until the owner approves again;
//! - a **requirement** names a kind of change that needs an approval
//!   before it lands: a new dependency, or any change under a path.
//!
//! harna cannot prove a human ran `harna approve` rather than an agent;
//! it keeps the command out of the MCP tools and says so in the docs.

use serde::{Deserialize, Serialize};

fn current() -> u32 {
    crate::SCHEMA_BASELINE
}

fn first() -> u32 {
    1
}

/// `.harna/approvals.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Approvals {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals: Vec<Approval>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub require: Vec<Require>,
}

/// One approval: a frozen path set, or a named thing (`dep:serde`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    /// Stable id: a component (`calendar`), a change (`migration-004`) or
    /// a dependency (`dep:<name>`).
    pub id: String,
    /// The files it freezes, as paths or globs. Empty for a dependency.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// Content hash of the files the paths matched when approved.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hash: String,
    /// Bumped on every re-approval: `calendar@3` is the third approved look.
    #[serde(default = "first")]
    pub version: u32,
    /// Who approved it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    /// When (UTC day).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub date: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// A screenshot of what was approved, if one was kept.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub screenshot: String,
}

/// A kind of change that needs an approval before it lands.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Require {
    /// One of `KINDS`.
    pub kind: String,
    /// For `path`: the globs whose changes need an approval. For
    /// `dependency-added`: optional manifest globs (all when empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// Why the owner wants to be asked.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// The kinds a requirement may name.
pub const KINDS: &[&str] = &["dependency-added", "path"];

impl Approval {
    /// `id@version`, the name a reviewer quotes.
    pub fn label(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    /// A dependency approval names a package, not files.
    pub fn dependency(&self) -> Option<&str> {
        self.id.strip_prefix("dep:")
    }
}

impl Require {
    pub fn line(&self) -> String {
        let what = match self.kind.as_str() {
            "dependency-added" => "a new dependency".to_string(),
            _ => format!("a change under {}", self.paths.join(", ")),
        };
        if self.reason.is_empty() {
            what
        } else {
            format!("{what} — {}", self.reason)
        }
    }
}

impl Approvals {
    pub fn is_empty(&self) -> bool {
        self.approvals.is_empty() && self.require.is_empty()
    }

    pub fn find(&self, id: &str) -> Option<&Approval> {
        self.approvals
            .iter()
            .find(|a| a.id.eq_ignore_ascii_case(id.trim()))
    }

    /// Record an approval; one with the same id is replaced at the next
    /// version. Returns the stored entry.
    pub fn approve(&mut self, mut a: Approval) -> &Approval {
        a.version = self.find(&a.id).map_or(1, |p| p.version + 1);
        self.approvals.retain(|x| !x.id.eq_ignore_ascii_case(&a.id));
        let at = self
            .approvals
            .iter()
            .position(|x| x.id > a.id)
            .unwrap_or(self.approvals.len());
        self.approvals.insert(at, a);
        &self.approvals[at]
    }

    /// Withdraw an approval; errs on an unknown id.
    pub fn revoke(&mut self, id: &str) -> anyhow::Result<()> {
        let before = self.approvals.len();
        self.approvals
            .retain(|x| !x.id.eq_ignore_ascii_case(id.trim()));
        if self.approvals.len() == before {
            anyhow::bail!("no approval `{id}` (harna approvals)");
        }
        Ok(())
    }

    /// Add a requirement (the same kind and paths replace it).
    pub fn require(&mut self, r: Require) -> anyhow::Result<()> {
        if !KINDS.contains(&r.kind.as_str()) {
            anyhow::bail!(
                "a requirement is one of {}, not `{}`",
                KINDS.join(" | "),
                r.kind
            );
        }
        if r.kind == "path" && r.paths.is_empty() {
            anyhow::bail!("a `path` requirement names the globs it covers (--paths 'db/**')");
        }
        self.require
            .retain(|x| !(x.kind == r.kind && x.paths == r.paths));
        self.require.push(r);
        Ok(())
    }
}

/// The briefing line: what is approved (and how much of it changed since),
/// and what needs the owner's yes first. Empty without a register.
pub fn brief(reg: &Approvals, changed: usize) -> String {
    let mut out = String::new();
    if !reg.approvals.is_empty() {
        out.push_str(&format!(
            "approvals: {} approved{} — a change to approved files waits for the owner's `harna approve`\n",
            reg.approvals.len(),
            if changed > 0 {
                format!(", {changed} CHANGED since")
            } else {
                String::new()
            }
        ));
    }
    if !reg.require.is_empty() {
        let lines: Vec<String> = reg.require.iter().map(Require::line).collect();
        out.push_str(&format!(
            "needs the owner's yes first (ask before, never approve yourself): {}\n",
            lines.join("; ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: &str) -> Approval {
        Approval {
            id: id.into(),
            paths: vec!["web/src/Calendar.tsx".into()],
            hash: "h".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_re_approval_is_the_next_version() {
        let mut r = Approvals::default();
        assert_eq!(r.approve(a("calendar")).label(), "calendar@1");
        assert_eq!(r.approve(a("button")).label(), "button@1");
        assert_eq!(r.approve(a("calendar")).label(), "calendar@2");
        assert_eq!(r.approvals.len(), 2);
        r.revoke("calendar").unwrap();
        assert!(r.revoke("calendar").is_err());
    }

    #[test]
    fn a_requirement_is_a_known_kind_and_a_path_one_names_paths() {
        let mut r = Approvals::default();
        let req = |kind: &str, paths: &[&str]| Require {
            kind: kind.into(),
            paths: paths.iter().map(|s| s.to_string()).collect(),
            reason: String::new(),
        };
        assert!(r.require(req("dependency-added", &[])).is_ok());
        assert!(r.require(req("path", &[])).is_err());
        assert!(r.require(req("deletion", &["x"])).is_err());
        r.require(req("path", &["src/billing/**"])).unwrap();
        r.require(req("path", &["src/billing/**"])).unwrap();
        assert_eq!(r.require.len(), 2, "the same requirement replaces itself");
    }

    #[test]
    fn the_briefing_names_what_changed_and_what_needs_a_yes() {
        let mut r = Approvals::default();
        assert_eq!(brief(&r, 0), "");
        r.approve(a("calendar"));
        r.require(Require {
            kind: "dependency-added".into(),
            paths: vec![],
            reason: String::new(),
        })
        .unwrap();
        let b = brief(&r, 1);
        assert!(b.contains("1 approved, 1 CHANGED since"), "{b}");
        assert!(b.contains("needs the owner's yes first"), "{b}");
        assert!(b.contains("a new dependency"), "{b}");
    }
}
