//! Human–agent scenarios: real collaboration stories replayed end to end.
//!
//! Each test is one story. What the human says becomes a tool call the
//! agent makes; every `session`/`one` is a fresh agent process (a new
//! context window), so whatever survives between steps survived because
//! harna stored it — not because a test kept it in memory. The assertions
//! are what the human would expect to see happen.

mod common;

use common::{call, initialized, is_error, one, session, session_env, text};
use serde_json::json;

/// "From now on, no unwrap() in library code."
/// The user states a preference once; tomorrow's agent knows it without
/// being told again — and knows WHY.
#[test]
fn a_preference_stated_once_survives_into_the_next_session() {
    let root = initialized("pref");
    // Day 1: the user says it, the agent persists it.
    let saved = one(&root, "harna_add_rule", json!({
        "id": "no-unwrap-in-lib",
        "text": "No unwrap() in library code; return errors",
        "rationale": "A panic in the library took the CLI down in production",
        "scope": ["crates/core"]
    }));
    assert!(text(&saved).contains("no-unwrap-in-lib"));

    // Day 2: a fresh agent opens the session.
    let onb = text(&one(&root, "harna_onboarding", json!({})));
    assert!(onb.contains("no-unwrap-in-lib"), "the rule rides onboarding:\n{onb}");
    assert!(onb.contains("took the CLI down"), "the reason travels with it");
    // Asked about that scope, the agent gets the rule; outside it, not.
    let scoped = text(&one(&root, "harna_rules", json!({"scope": "crates/core/src"})));
    assert!(scoped.contains("no-unwrap-in-lib"));
    let elsewhere = text(&one(&root, "harna_rules", json!({"scope": "crates/cli"})));
    assert!(!elsewhere.contains("no-unwrap-in-lib"), "{elsewhere}");
}

/// "We're not doing a web dashboard. Ever."
/// Next week a different agent is asked to plan "a web dashboard for the
/// metrics" — the refusal speaks before any code is proposed.
#[test]
fn a_refused_feature_is_flagged_when_a_later_agent_plans_it() {
    let root = initialized("refusal");
    one(&root, "harna_non_goal", json!({
        "action": "add", "id": "no-web-dashboard",
        "refusal": "no web dashboard for metrics",
        "reason": "harna stays a local CLI; a server is an ops burden we refused"
    }));
    let plan = text(&one(&root, "harna_place", json!({"idea": "web dashboard for metrics"})));
    assert!(plan.contains("no-web-dashboard"), "the refusal is named:\n{plan}");
    assert!(plan.contains("ops burden"), "with the user's reason");
}

/// "We tried caching the map in SQLite last month — it was slower."
/// The user's war story becomes a lesson; when an agent later considers
/// the same direction, the dead end and its evidence come up first.
#[test]
fn a_dead_end_is_surfaced_before_it_is_re_suggested() {
    let root = initialized("lesson");
    one(&root, "harna_lesson", json!({
        "action": "add", "id": "sqlite-map-cache",
        "claim": "cache the code map in sqlite",
        "verdict": "slower than the json shards for every repo we measured",
        "evidence": ["bench 2026-09: 3.1s vs 0.4s"]
    }));
    let plan = text(&one(&root, "harna_place", json!({"idea": "sqlite cache for the code map"})));
    assert!(plan.contains("sqlite-map-cache"), "{plan}");
    assert!(plan.contains("3.1s vs 0.4s"), "the evidence, not just the verdict");
}

/// The context window runs out mid-task. The agent records where it was;
/// the next agent — which knows nothing — picks up exactly there, and
/// clears the note once it has resumed.
#[test]
fn compaction_mid_task_resumes_from_the_handoff() {
    let root = initialized("handoff");
    one(&root, "harna_handoff", json!({
        "summary": "Half-way through moving auth into its own container.\nDone: types moved.\nNot done: callers.",
        "next_steps": ["update the 3 callers in api/", "re-run validate --changed"]
    }));
    // Fresh agent, zero memory.
    let onb = text(&one(&root, "harna_onboarding", json!({})));
    assert!(onb.contains("moving auth into its own container"), "{onb}");
    assert!(onb.contains("update the 3 callers in api/"));
    // Resumed: the note is cleared so it never misleads a later session.
    one(&root, "harna_handoff", json!({"clear": true}));
    let later = text(&one(&root, "harna_onboarding", json!({})));
    assert!(!later.contains("moving auth"), "a stale handoff must not linger");
}

/// "Actually, forget that rule — we allow unwrap in tests now."
/// The user changes their mind; the rule is retired visibly with the
/// reason, and agents stop enforcing it.
#[test]
fn a_reversed_decision_retires_the_rule_visibly() {
    let root = initialized("reverse");
    one(&root, "harna_add_rule", json!({
        "id": "no-unwrap", "text": "No unwrap() anywhere", "rationale": "panics"
    }));
    let r = one(&root, "harna_rule_lifecycle", json!({
        "id": "no-unwrap", "action": "deprecate", "reason": "user: unwrap is fine in tests"
    }));
    assert!(!is_error(&r), "{r}");
    let rules = std::fs::read_to_string(root.join(".harna/rules.toml")).unwrap();
    assert!(rules.contains("unwrap is fine in tests"), "the why is on record:\n{rules}");
    // Briefed as visibly dead, with the reason — so an agent that remembers
    // the old rule from a stale doc is told explicitly to stop following it.
    let onb = text(&one(&root, "harna_onboarding", json!({})));
    assert!(onb.contains("[no-unwrap] (DEPRECATED — do not follow)"), "{onb}");
    assert!(onb.contains("unwrap is fine in tests"));
}

/// A brand-new contributor's agent lands in a repo nobody has explained
/// to it, on a lean `core` tool profile. It still learns the vocabulary
/// and can orient, plan and check — the ritual fits in the small surface.
#[test]
fn a_new_agent_on_the_core_profile_can_still_walk_the_ritual() {
    let root = initialized("newcomer");
    let core = [("HARNA_TOOLS", "core")];
    let r = session_env(&root, &core, &[
        call("harna_onboarding", json!({})),
        call("harna_status", json!({})),
        call("harna_place", json!({"idea": "retry policy for http calls"})),
        call("harna_validate", json!({})),
        call("harna_handoff", json!({"summary": "explored; nothing changed"})),
        call("harna_roadmap", json!({"action": "list"})),
    ]);
    let onb = text(&r[0]);
    assert!(onb.contains("concepts: harna's vocabulary"), "vocabulary arrives once:\n{onb}");
    assert!(onb.contains("parity = "));
    for step in &r[1..5] {
        assert!(!is_error(step), "ritual step failed on core: {step}");
    }
    assert!(text(&r[3]).contains("verdict: PASS"));
    // Outside the profile: not advertised, so the call is refused —
    // the agent knows to ask for `full` rather than guess.
    assert!(r[5].get("error").is_some() || is_error(&r[5]), "{}", r[5]);
}

/// The user asks the agent to "just mark that warning as fine". An Info
/// can be accepted with a reason; an Error cannot — however it is asked.
#[test]
fn accepting_a_warning_never_silences_an_error() {
    let root = initialized("accept");
    let r = session(&root, &[
        call("harna_acknowledge", json!({
            "action": "add", "check": "hotspot", "file": "src/big.rs",
            "reason": "user: splitting it is scheduled for 0.3.37"
        })),
        call("harna_acknowledge", json!({"action": "list"})),
        call("harna_acknowledge", json!({"action": "add", "check": "hotspot", "file": "src/big.rs"})),
    ]);
    assert!(text(&r[1]).contains("scheduled for 0.3.37"));
    assert!(is_error(&r[2]), "no reason, no acceptance: {}", r[2]);
}
