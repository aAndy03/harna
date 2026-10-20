//! Component `doctor`: the installation self-audit.
//!
//! One command answers "is harna healthy in this repo?" — schema currency,
//! data-file parseability, map freshness, rules, git, hook gate, agent
//! wiring. Every check says one word and offers the next action; FAIL only
//! when the installation genuinely blocks work (scripts can gate on it).

use std::path::Path;

use serde::Serialize;

use crate::schema::store::Store;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Grade {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckReport {
    /// short check id, e.g. "schema".
    pub check: String,
    pub grade: Grade,
    pub message: String,
    /// Suggested next action when grade != Ok.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub version: String,
    pub checks: Vec<CheckReport>,
}

impl DoctorReport {
    pub fn fail_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.grade == Grade::Fail)
            .count()
    }

    pub fn warn_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.grade == Grade::Warn)
            .count()
    }

    pub fn render(&self) -> String {
        let mut out = format!("harna doctor (tool {})\n", self.version);
        for c in &self.checks {
            let mark = match c.grade {
                Grade::Ok => "ok  ",
                Grade::Warn => "warn",
                Grade::Fail => "FAIL",
            };
            out.push_str(&format!("  {mark}  {:14} {}\n", c.check, c.message));
            if !c.action.is_empty() {
                out.push_str(&format!("      -> {}\n", c.action));
            }
        }
        out.push_str(&format!(
            "\nsummary: {} ok, {} warn, {} fail\n",
            self.checks.iter().filter(|c| c.grade == Grade::Ok).count(),
            self.warn_count(),
            self.fail_count()
        ));
        out
    }
}

fn ok(check: &str, message: &str) -> CheckReport {
    CheckReport {
        check: check.into(),
        grade: Grade::Ok,
        message: message.into(),
        action: String::new(),
    }
}

fn warn(check: &str, message: &str, action: &str) -> CheckReport {
    CheckReport {
        check: check.into(),
        grade: Grade::Warn,
        message: message.into(),
        action: action.into(),
    }
}

fn fail(check: &str, message: &str, action: &str) -> CheckReport {
    CheckReport {
        check: check.into(),
        grade: Grade::Fail,
        message: message.into(),
        action: action.into(),
    }
}

/// Binding coherence (doctor level): every bound check id resolves in the
/// registry and every severity value is one of the accepted words. A
/// deprecated rule is dead and owes nothing.
fn rules_coherence(rules: &crate::schema::model::rules::Rules) -> CheckReport {
    use crate::enforcement::checks as registry;

    let mut problems = Vec::new();
    let mut bindings = 0usize;
    for r in &rules.rules {
        if r.is_deprecated() {
            continue;
        }
        if let Some(c) = &r.check {
            bindings += 1;
            if !registry::known(c) {
                problems.push(format!("rule `{}` binds unknown check `{c}`", r.id));
            }
        }
        if let Some(s) = &r.severity {
            if registry::parse_severity(s).is_none() {
                problems.push(format!("rule `{}` sets unknown severity `{s}`", r.id));
            } else if r.check.is_none() && r.pattern.is_none() {
                problems.push(format!("rule `{}` sets severity with no bound check", r.id));
            }
        }
        if let Some(p) = &r.pattern {
            if let Err(e) = registry::compile_pattern(p) {
                problems.push(format!(
                    "rule `{}` has an un-compilable pattern `{p}`: {e}",
                    r.id
                ));
            }
            if let Some(c) = &r.check {
                if c != "pattern" {
                    problems.push(format!(
                        "rule `{}` carries a pattern but binds check `{c}` (pattern rules run the `pattern` check)",
                        r.id
                    ));
                }
            }
        }
    }
    if problems.is_empty() && bindings > 0 {
        ok(
            "rule-bindings",
            &format!("{} check binding(s) resolve", bindings),
        )
    } else if problems.is_empty() {
        ok("rule-bindings", "no bindings (all rules advisory)")
    } else {
        warn(
            "rule-bindings",
            &format!(
                "{} binding problem(s): {}",
                problems.len(),
                problems.join("; ")
            ),
            &format!(
                "fix the check ids / severity values in .harna/rules.toml (registry: {})",
                registry::known_ids_listed()
            ),
        )
    }
}

/// Run every installation check. `root` is the repo root.
pub fn build(store: &Store, root: &Path) -> anyhow::Result<DoctorReport> {
    let mut checks = Vec::new();

    // 1) .harna present; manifest + rules parseable; schema currency.
    if !store.exists() {
        checks.push(fail(
            ".harna",
            "missing — this repo has no harna memory",
            "run `harna init`, then `harna scan`",
        ));
        return Ok(DoctorReport {
            version: env!("CARGO_PKG_VERSION").into(),
            checks,
        });
    }
    checks.push(ok(".harna", "present"));

    // Manifest: parse WITHOUT the schema guard so old/new data speaks here.
    let manifest_raw = std::fs::read_to_string(store.manifest_path()).unwrap_or_default();
    let manifest: Option<crate::schema::model::manifest::Manifest> =
        toml::from_str(&manifest_raw).ok();
    match manifest {
        Some(m) => {
            if m.schema_version > crate::SCHEMA_VERSION {
                checks.push(fail(
                    "schema",
                    &format!(
                        "repo data is schema v{} but this harna knows v{}",
                        m.schema_version,
                        crate::SCHEMA_VERSION
                    ),
                    "update harna before touching this repository",
                ));
            } else if m.schema_version < crate::SCHEMA_VERSION {
                checks.push(warn(
                    "schema",
                    &format!(
                        "repo data at schema v{}, tool knows v{}",
                        m.schema_version,
                        crate::SCHEMA_VERSION
                    ),
                    "run `harna migrate`",
                ));
            } else {
                checks.push(ok("schema", "current"));
            }
        }
        None => checks.push(fail(
            "manifest",
            "manifest.toml is missing or unparseable",
            "restore the file from git, or re-run `harna init`",
        )),
    }

    // Rules parseable — plus binding coherence against the check registry:
    // the same canonical source the validators, the doctor and the CLI/MCP
    // add-rule edges read, so a drifting hand-edit is visible at session
    // start, not only at the next validate.
    match store.load_rules() {
        Ok(r) => {
            checks.push(ok("rules", &format!("{} recorded", r.rules.len())));
            checks.push(rules_coherence(&r));
        }
        Err(e) => checks.push(fail(
            "rules",
            &format!("rules.toml unparseable: {e:#}"),
            "restore from git or fix the syntax",
        )),
    }

    // Glossary: parseable and collision-coherent when recorded (0.3.26).
    // A corrupt or colliding glossary teaches agents the wrong vocabulary
    // at every session start, so parse failures FAIL, collisions WARN.
    match store.load_terms() {
        Ok(t) if t.is_empty() => checks.push(ok("terms", "no glossary recorded (optional)")),
        Ok(t) => match t.alias_collisions() {
            collisions if collisions.is_empty() => {
                checks.push(ok("terms", &format!("{} term(s), coherent", t.terms.len())));
            }
            collisions => {
                let named = collisions
                    .iter()
                    .map(|(owners, spelling)| format!("{owners} share {spelling}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                checks.push(warn(
                    "terms",
                    &format!("{} alias collision(s): {}", collisions.len(), named),
                    "edit .harna/terms.toml — one spelling may not mean two things; split the alias or merge the terms",
                ));
            }
        },
        Err(e) => checks.push(fail(
            "terms",
            &format!("terms.toml unparseable: {e:#}"),
            "fix or delete .harna/terms.toml",
        )),
    }

    // Decline register (0.3.27): deliberate refusals — parseable when
    // recorded; a corrupt one would silently un-refuse refused work.
    match store.load_non_goals() {
        Ok(g) if g.is_empty() => checks.push(ok("non-goals", "none recorded (optional)")),
        Ok(g) => {
            let hollow = g
                .goals
                .iter()
                .filter(|n| n.refusal.trim().is_empty())
                .count();
            if hollow == 0 {
                checks.push(ok(
                    "non-goals",
                    &format!("{} refusal(s) in force", g.goals.len()),
                ));
            } else {
                checks.push(warn(
                    "non-goals",
                    &format!("{hollow} refusal(s) have empty refusal text"),
                    "edit .harna/non-goals.toml — a refusal must say what is refused",
                ));
            }
        }
        Err(e) => checks.push(fail(
            "non-goals",
            &format!("non-goals.toml unparseable: {e:#}"),
            "fix or delete .harna/non-goals.toml",
        )),
    }

    // Dead-ends register (0.3.27): recorded failures — parseable when
    // recorded; a corrupt one would let dead ends be re-suggested.
    match store.load_lessons() {
        Ok(l) if l.is_empty() => checks.push(ok("lessons", "none recorded (optional)")),
        Ok(l) => {
            let hollow = l
                .lessons
                .iter()
                .filter(|x| x.verdict.trim().is_empty())
                .count();
            if hollow == 0 {
                checks.push(ok(
                    "lessons",
                    &format!("{} dead end(s) recorded", l.lessons.len()),
                ));
            } else {
                checks.push(warn(
                    "lessons",
                    &format!("{hollow} lesson(s) carry no verdict"),
                    "edit .harna/lessons.toml — a lesson without a verdict teaches nothing",
                ));
            }
        }
        Err(e) => checks.push(fail(
            "lessons",
            &format!("lessons.toml unparseable: {e:#}"),
            "fix or delete .harna/lessons.toml",
        )),
    }

    // Qualities (0.4.0): long-term intent — parseable when declared; a
    // quality without a statement says nothing.
    match store.load_qualities() {
        Ok(q) if q.is_empty() => checks.push(ok("qualities", "none declared (optional)")),
        Ok(q) => {
            let hollow = q
                .qualities
                .iter()
                .filter(|x| x.statement.trim().is_empty())
                .count();
            if hollow == 0 {
                checks.push(ok(
                    "qualities",
                    &format!("{} quality(ies) declared", q.qualities.len()),
                ));
            } else {
                checks.push(warn(
                    "qualities",
                    &format!("{hollow} quality(ies) carry no statement"),
                    "harna quality add <id> --statement ... — an empty quality guides nothing",
                ));
            }
        }
        Err(e) => checks.push(fail(
            "qualities",
            &format!("qualities.toml unparseable: {e:#}"),
            "fix or delete .harna/qualities.toml",
        )),
    }

    // Roadmap (0.3.28): stated data — parseable when declared; a
    // corrupt one would teach a future it does not actually have.
    match store.load_roadmap() {
        Ok(r) if r.is_empty() => checks.push(ok("roadmap", "none declared (optional)")),
        Ok(r) => {
            let problems = r.coherence();
            if problems.is_empty() {
                let now = r.current().map(|c| format!("now is `{}`", c.id));
                let summary = now.unwrap_or_else(|| {
                    format!("{} milestone(s) declared, none current", r.milestones.len())
                });
                checks.push(ok("roadmap", &summary));
            } else {
                checks.push(warn(
                    "roadmap",
                    &problems.join("; "),
                    "fix .harna/roadmap.toml — declared data must stay coherent",
                ));
            }
        }
        Err(e) => checks.push(fail(
            "roadmap",
            &format!("roadmap.toml unparseable: {e:#}"),
            "fix or delete .harna/roadmap.toml",
        )),
    }

    // Advisory ledger (0.3.31): accepted advisories — parseable when
    // locked; a corrupt ledger would silently re-arm everything (noisy
    // but safe) or, worse, be trusted blindly. Name the damage.
    match store.load_acknowledged() {
        Ok(a) if a.is_empty() => checks.push(ok("acknowledged", "none locked (optional)")),
        Ok(a) => checks.push(ok(
            "acknowledged",
            &format!("{} advisories locked with reasons", a.entries.len()),
        )),
        Err(e) => checks.push(fail(
            "acknowledged",
            &format!("acknowledged.toml unparseable: {e:#}"),
            "fix or delete .harna/acknowledged.toml",
        )),
    }

    // Decisions directory.
    if store.decisions_dir().is_dir() {
        let count = std::fs::read_dir(store.decisions_dir())
            .map(|d| d.flatten().count())
            .unwrap_or(0);
        checks.push(ok("decisions", &format!("{count} recorded")));
    } else {
        checks.push(warn(
            "decisions",
            "directory missing",
            "log your first decision with `harna log-decision`",
        ));
    }

    // Handoff: parseable when present — a corrupt burst journal would
    // silently strand the "where was I?" answer at session start.
    match crate::schema::journal::handoff::load(store) {
        Some(h) => checks.push(ok(
            "handoff",
            &format!("{} next step(s) recorded", h.declared_next.len()),
        )),
        None => {
            if crate::schema::journal::handoff::path(store).exists() {
                checks.push(fail(
                    "handoff",
                    "handoff.toml is unparseable",
                    "fix or delete .harna/handoff.toml",
                ));
            } else {
                checks.push(ok("handoff", "none recorded (optional)"));
            }
        }
    }

    // Digest freshness: present, parseable, not outrun by changed files.
    match store.load_digest() {
        // A map written by a newer harna than the one running (0.3.47):
        // hooks call whatever `harna` is on PATH, and an older binary
        // misreads a newer map (this repo's 0.3.41 hook reported cycles
        // the 0.3.46 extractor had already dissolved).
        Ok(d) if d.extractor_version > crate::search::digest::EXTRACTOR_VERSION => {
            checks.push(warn(
                "binary-version",
                &format!(
                    "this harna ({}) is older than the one that wrote the map (extractor {} > {}) — its readings are stale",
                    env!("CARGO_PKG_VERSION"),
                    d.extractor_version,
                    crate::search::digest::EXTRACTOR_VERSION
                ),
                "update the installed binary (`cargo install harna-cli`, or `--path crates/harna-cli` in a harna checkout) — hooks run the one on PATH",
            ));
        }
        Ok(d) => {
            let stamp = chrono::DateTime::parse_from_rfc3339(&d.generated_at).ok();
            let stale_files = stamp
                .map(|st| {
                    crate::search::digest::respected_rust_files(root, 50_000)
                        .into_iter()
                        .filter(|rel| {
                            std::fs::metadata(root.join(rel))
                                .and_then(|m| m.modified())
                                .map(|mt| {
                                    let utc: chrono::DateTime<chrono::Utc> = mt.into();
                                    utc > st.with_timezone(&chrono::Utc)
                                })
                                .unwrap_or(false)
                        })
                        .count()
                })
                .unwrap_or(0);
            if stale_files == 0 {
                checks.push(ok(
                    "digest",
                    &format!("{} modules, current", d.module_count()),
                ));
            } else {
                checks.push(warn(
                    "digest",
                    &format!("{} module(s) changed since the last scan", stale_files),
                    "run `harna scan` (incremental — only the changed files are re-parsed)",
                ));
            }
        }
        Err(_) => checks.push(warn("digest", "missing or unparseable", "run `harna scan`")),
    }

    // Git: parity, dependents, diff, and --changed all lean on it.
    let git_ok = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if git_ok {
        checks.push(ok("git", "repository present"));
    } else {
        checks.push(warn(
            "git",
            "no repository — parity events, dependents diff and --changed need it",
            "run `git init` and commit .harna/ with the code it describes",
        ));
    }

    // Commit gate: does the pre-commit hook invoke harna?
    if git_ok {
        let hooks_path = std::process::Command::new("git")
            .args(["config", "core.hooksPath"])
            .current_dir(root)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        let hook_dir = hooks_path.unwrap_or_default();
        let hook_file = if hook_dir.is_empty() {
            root.join(".git/hooks/pre-commit")
        } else {
            root.join(&hook_dir).join("pre-commit")
        };
        match std::fs::read_to_string(&hook_file) {
            Ok(body) if body.contains("harna") || body.contains("harna.exe") => {
                checks.push(ok("commit-gate", "pre-commit hook wired to harna"));
            }
            _ => checks.push(warn(
                "commit-gate",
                "no pre-commit hook invoking harna",
                "run `harna hooks install` (git pre-commit + core.hooksPath, Claude Code hooks, CI)",
            )),
        }
    }

    // Agent instructions: does AGENTS.md exist and speak about harna?
    let agents = std::fs::read_to_string(root.join("AGENTS.md"));
    match agents {
        Ok(body) if body.to_lowercase().contains("harna") => {
            checks.push(ok(
                "agent-instructions",
                "AGENTS.md carries the harna contract",
            ));
        }
        Ok(_) => checks.push(warn(
            "agent-instructions",
            "AGENTS.md exists but does not mention harna",
            "run `harna init --agents` to append the managed harna block (your prose stays)",
        )),
        Err(_) => checks.push(warn(
            "agent-instructions",
            "no AGENTS.md — agents start every session blind",
            "run `harna init --agents` (writes the stock instruction file)",
        )),
    }

    // Agent wiring: is harna referenced by a local MCP config, or is the
    // MCP server binary installed? 0.3.32 dogfood: the warn was
    // unsatisfiable on repos whose agents use the CLI — the server ships
    // (harna-mcp), it just was never looked for.
    let wired = ["opencode.json", ".mcp.json"]
        .iter()
        .map(|f| root.join(f))
        .any(|p| {
            std::fs::read_to_string(p)
                .map(|body| body.contains("harna"))
                .unwrap_or(false)
        });
    let server_installed = std::process::Command::new("harna-mcp")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if wired {
        checks.push(ok("agent-wiring", "local MCP config mentions harna"));
    } else if server_installed {
        checks.push(ok(
            "agent-wiring",
            "harna-mcp server binary on PATH (one config line wires it)",
        ));
    } else {
        checks.push(warn(
            "agent-wiring",
            "no local MCP config mentions harna and no harna-mcp binary on PATH",
            "install harna-mcp alongside harna, then add the MCP server to opencode.json / .mcp.json so agents load the tools",
        ));
    }

    Ok(DoctorReport {
        version: env!("CARGO_PKG_VERSION").into(),
        checks,
    })
}

#[cfg(test)]
mod tests;
