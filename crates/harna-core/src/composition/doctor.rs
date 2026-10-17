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
        self.checks.iter().filter(|c| c.grade == Grade::Fail).count()
    }

    pub fn warn_count(&self) -> usize {
        self.checks.iter().filter(|c| c.grade == Grade::Warn).count()
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
fn rules_coherence(rules: &crate::schema::rules::Rules) -> CheckReport {
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
            &format!("{} binding problem(s): {}", problems.len(), problems.join("; ")),
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
        return Ok(DoctorReport { version: env!("CARGO_PKG_VERSION").into(), checks });
    }
    checks.push(ok(".harna", "present"));

    // Manifest: parse WITHOUT the schema guard so old/new data speaks here.
    let manifest_raw = std::fs::read_to_string(store.manifest_path()).unwrap_or_default();
    let manifest: Option<crate::schema::manifest::Manifest> =
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
            let hollow = g.goals.iter().filter(|n| n.refusal.trim().is_empty()).count();
            if hollow == 0 {
                checks.push(ok("non-goals", &format!("{} refusal(s) in force", g.goals.len())));
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
            let hollow = l.lessons.iter().filter(|x| x.verdict.trim().is_empty()).count();
            if hollow == 0 {
                checks.push(ok("lessons", &format!("{} dead end(s) recorded", l.lessons.len())));
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
    match crate::schema::handoff::load(store) {
        Some(h) => checks.push(ok(
            "handoff",
            &format!(
                "{} next step(s) recorded",
                h.declared_next.len()
            ),
        )),
        None => {
            if crate::schema::handoff::path(store).exists() {
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
        Err(_) => checks.push(warn(
            "digest",
            "missing or unparseable",
            "run `harna scan`",
        )),
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
                "wire `.githooks/pre-commit` to `harna validate --changed` and set core.hooksPath",
            )),
        }
    }

    // Agent instructions: does AGENTS.md exist and speak about harna?
    let agents = std::fs::read_to_string(root.join("AGENTS.md"));
    match agents {
        Ok(body) if body.to_lowercase().contains("harna") => {
            checks.push(ok("agent-instructions", "AGENTS.md carries the harna contract"));
        }
        Ok(_) => checks.push(warn(
            "agent-instructions",
            "AGENTS.md exists but does not mention harna",
            "run `harna init` to append the section, or paste it manually",
        )),
        Err(_) => checks.push(warn(
            "agent-instructions",
            "no AGENTS.md — agents start every session blind",
            "run `harna init` (writes the stock instruction file)",
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

    Ok(DoctorReport { version: env!("CARGO_PKG_VERSION").into(), checks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::store::Store;

    #[test]
    fn fresh_repo_is_healthy_with_expected_warnings() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        let r = build(&store, &tmp)?;
        let find = |name: &str| r.checks.iter().find(|c| c.check == name).unwrap();
        assert_eq!(find(".harna").grade, Grade::Ok);
        assert_eq!(find("schema").grade, Grade::Ok);
        assert_eq!(find("rules").grade, Grade::Ok);
        assert_eq!(find("digest").grade, Grade::Warn, "no scan yet");
        // No burst journal yet: optional, so Ok rather than a warning.
        assert_eq!(
            find("handoff").grade,
            Grade::Ok,
            "no handoff recorded is a legitimate state"
        );
        assert_eq!(r.fail_count(), 0, "a fresh init must not fail");

        // A corrupt handoff must fail loudly: it would strand the "where
        // was I?" answer silently at every session start.
        std::fs::write(
            crate::schema::handoff::path(&store),
            "not = [valid toml {{{{",
        )
        .unwrap();
        let r2 = build(&store, &tmp)?;
        let handoff2 = r2.checks.iter().find(|c| c.check == "handoff").unwrap();
        assert_eq!(handoff2.grade, Grade::Fail);
        Ok(())
    }

    #[test]
    fn corrupted_rules_fail_but_do_not_stop_other_checks() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        std::fs::write(store.rules_path(), "not = [valid toml {{{{").unwrap();

        let r = build(&store, &tmp)?;
        assert_eq!(r.fail_count(), 1);
        assert_eq!(r.checks.iter().filter(|c| c.check == "rules").count(), 1);
        let rules_check = r.checks.iter().find(|c| c.check == "rules").unwrap();
        assert_eq!(rules_check.grade, Grade::Fail);
        // The doctor still reports the rest.
        assert!(r.checks.iter().any(|c| c.check == "schema"));
        Ok(())
    }

    #[test]
    fn the_glossary_is_audited_when_recorded() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor6-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        // Absent: optional, so Ok rather than a warning.
        let r = build(&store, &tmp)?;
        assert_eq!(r.checks.iter().find(|c| c.check == "terms").unwrap().grade, Grade::Ok);

        // Corrupt: fails loudly — a silent glossary would teach the wrong
        // vocabulary at every session start.
        std::fs::write(store.terms_path(), "not = [valid toml {{{{").unwrap();
        let r2 = build(&store, &tmp)?;
        let terms2 = r2.checks.iter().find(|c| c.check == "terms").unwrap();
        assert_eq!(terms2.grade, Grade::Fail);
        assert!(terms2.action.contains("terms.toml"));

        // Coherent: named terms counted.
        let terms = crate::schema::terms_data::Terms {
            schema_version: crate::SCHEMA_VERSION,
            terms: vec![crate::schema::terms_data::Term {
                word: "engine".into(),
                aliases: vec!["service".into()],
                means: "a container that carries the engine role".into(),
            }],
        };
        store.save_terms(&terms).unwrap();
        let r3 = build(&store, &tmp)?;
        let terms3 = r3.checks.iter().find(|c| c.check == "terms").unwrap();
        assert_eq!(terms3.grade, Grade::Ok);
        assert!(terms3.message.contains("1 term(s)"));

        // Two spellings one meaning: warned, named by owners.
        let alerted = crate::schema::terms_data::Terms {
            schema_version: crate::SCHEMA_VERSION,
            terms: vec![
                crate::schema::terms_data::Term {
                    word: "engine".into(),
                    aliases: vec!["service".into()],
                    means: "a container that carries the engine role".into(),
                },
                crate::schema::terms_data::Term {
                    word: "gateway".into(),
                    aliases: vec!["service".into()],
                    means: "a facility monopoly (fs | net | process | base)".into(),
                },
            ],
        };
        store.save_terms(&alerted).unwrap();
        let r4 = build(&store, &tmp)?;
        let terms4 = r4.checks.iter().find(|c| c.check == "terms").unwrap();
        assert_eq!(terms4.grade, Grade::Warn);
        assert!(terms4.message.contains("alias collision"));
        assert!(terms4.message.contains("engine"));
        assert!(terms4.message.contains("gateway"));
        Ok(())
    }

    #[test]
    fn future_schema_is_a_fail_with_update_hint() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        std::fs::write(store.manifest_path(), "schema_version = 999\n").unwrap();

        let r = build(&store, &tmp)?;
        let schema = r.checks.iter().find(|c| c.check == "schema").unwrap();
        assert_eq!(schema.grade, Grade::Fail);
        assert!(schema.action.contains("update harna"));
        Ok(())
    }

    #[test]
    fn drifting_bindings_warn_but_do_not_fail() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        std::fs::write(
            store.rules_path(),
            "schema_version = 1\n\n[[rules]]\nid = \"t\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\ncheck = \"no-such-check\"\nseverity = \"warn\"\n",
        )
        .unwrap();

        let r = build(&store, &tmp)?;
        let binding = r.checks.iter().find(|c| c.check == "rule-bindings").unwrap();
        assert_eq!(binding.grade, Grade::Warn);
        assert!(binding.message.contains("2 binding problem(s)"));
        assert!(binding.action.contains("rules.toml"));
        assert_eq!(r.fail_count(), 0, "binding drift advises, it must not fail");
        Ok(())
    }

    #[test]
    fn decline_and_deadend_registers_are_audited() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor7-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        // Absent: optional, Ok.
        let r = build(&store, &tmp)?;
        assert_eq!(r.checks.iter().find(|c| c.check == "non-goals").unwrap().grade, Grade::Ok);
        assert_eq!(r.checks.iter().find(|c| c.check == "lessons").unwrap().grade, Grade::Ok);

        // Corrupt: fails loudly — a silent decline register would
        // silently un-refuse refused work.
        std::fs::write(store.non_goals_path(), "not = [valid toml {{{{").unwrap();
        std::fs::write(store.lessons_path(), "not = [valid toml {{{{").unwrap();
        let r2 = build(&store, &tmp)?;
        assert_eq!(r2.checks.iter().find(|c| c.check == "non-goals").unwrap().grade, Grade::Fail);
        assert_eq!(r2.checks.iter().find(|c| c.check == "lessons").unwrap().grade, Grade::Fail);

        // A verdict-less lesson is WARNED, not dead-silent.
        let mut l = crate::schema::lessons_data::Lessons::default();
        l.upsert(crate::schema::lessons_data::Lesson {
            id: "thin".into(),
            claim: "tried once".into(),
            verdict: String::new(),
            evidence: vec![],
            recorded: "2026-10-17".into(),
        });
        store.save_lessons(&l).unwrap();
        std::fs::remove_file(store.non_goals_path()).unwrap();
        let r3 = build(&store, &tmp)?;
        let lessons3 = r3.checks.iter().find(|c| c.check == "lessons").unwrap();
        assert_eq!(lessons3.grade, Grade::Warn);
        assert!(lessons3.message.contains("1 lesson(s) carry no verdict"));
        Ok(())
    }

    #[test]
    fn the_roadmap_is_audited_when_declared() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor8-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        // Not solved: optional, Ok.
        let r = build(&store, &tmp)?;
        assert_eq!(
            r.checks.iter().find(|c| c.check == "roadmap").unwrap().grade,
            Grade::Ok
        );

        // Declared + coherent: "my current situation" is named.
        let mut rdoc = crate::schema::roadmap_data::Roadmap::default();
        rdoc.upsert(crate::schema::roadmap_data::Milestone {
            id: "0.3.28".into(),
            title: "roadmap as data".into(),
            state: "current".into(),
            notes: String::new(),
        });
        store.save_roadmap(&rdoc).unwrap();
        let r2 = build(&store, &tmp)?;
        let road2 = r2.checks.iter().find(|c| c.check == "roadmap").unwrap();
        assert_eq!(road2.grade, Grade::Ok);
        assert!(road2.message.contains("now is `0.3.28`"), "{}", road2.message);

        // Incoherent declaration: warned, not dead-silent.
        rdoc.milestones.push(crate::schema::roadmap_data::Milestone {
            id: "0.3.29".into(),
            title: "dup".into(),
            state: "current".into(),
            notes: String::new(),
        });
        store.save_roadmap(&rdoc).unwrap();
        let r3 = build(&store, &tmp)?;
        let road3 = r3.checks.iter().find(|c| c.check == "roadmap").unwrap();
        assert_eq!(road3.grade, Grade::Warn);
        assert!(road3.message.contains("2 milestones marked `current`"));
        Ok(())
    }

    #[test]
    fn coherent_binding_is_ok() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        std::fs::write(
            store.rules_path(),
            "schema_version = 1\n\n[[rules]]\nid = \"t\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\ncheck = \"layer-boundary\"\nseverity = \"info\"\n",
        )
        .unwrap();

        let r = build(&store, &tmp)?;
        let binding = r.checks.iter().find(|c| c.check == "rule-bindings").unwrap();
        assert_eq!(binding.grade, Grade::Ok, "override included");
        Ok(())
    }
}
