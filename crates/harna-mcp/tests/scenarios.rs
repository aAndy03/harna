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
    let saved = one(
        &root,
        "harna_add_rule",
        json!({
            "id": "no-unwrap-in-lib",
            "text": "No unwrap() in library code; return errors",
            "rationale": "A panic in the library took the CLI down in production",
            "scope": ["crates/core"]
        }),
    );
    assert!(text(&saved).contains("no-unwrap-in-lib"));

    // Day 2: a fresh agent opens the session.
    let onb = text(&one(&root, "harna_onboarding", json!({})));
    assert!(
        onb.contains("no-unwrap-in-lib"),
        "the rule rides onboarding:\n{onb}"
    );
    assert!(
        onb.contains("took the CLI down"),
        "the reason travels with it"
    );
    // Asked about that scope, the agent gets the rule; outside it, not.
    let scoped = text(&one(
        &root,
        "harna_rules",
        json!({"scope": "crates/core/src"}),
    ));
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
    one(
        &root,
        "harna_non_goal",
        json!({
            "action": "add", "id": "no-web-dashboard",
            "refusal": "no web dashboard for metrics",
            "reason": "harna stays a local CLI; a server is an ops burden we refused"
        }),
    );
    let plan = text(&one(
        &root,
        "harna_place",
        json!({"idea": "web dashboard for metrics"}),
    ));
    assert!(
        plan.contains("no-web-dashboard"),
        "the refusal is named:\n{plan}"
    );
    assert!(plan.contains("ops burden"), "with the user's reason");
}

/// "We tried caching the map in SQLite last month — it was slower."
/// The user's war story becomes a lesson; when an agent later considers
/// the same direction, the dead end and its evidence come up first.
#[test]
fn a_dead_end_is_surfaced_before_it_is_re_suggested() {
    let root = initialized("lesson");
    one(
        &root,
        "harna_lesson",
        json!({
            "action": "add", "id": "sqlite-map-cache",
            "claim": "cache the code map in sqlite",
            "verdict": "slower than the json shards for every repo we measured",
            "evidence": ["bench 2026-09: 3.1s vs 0.4s"]
        }),
    );
    let plan = text(&one(
        &root,
        "harna_place",
        json!({"idea": "sqlite cache for the code map"}),
    ));
    assert!(plan.contains("sqlite-map-cache"), "{plan}");
    assert!(
        plan.contains("3.1s vs 0.4s"),
        "the evidence, not just the verdict"
    );
}

/// The context window runs out mid-task. The agent records where it was;
/// the next agent — which knows nothing — picks up exactly there, and
/// clears the note once it has resumed.
#[test]
fn compaction_mid_task_resumes_from_the_handoff() {
    let root = initialized("handoff");
    one(
        &root,
        "harna_handoff",
        json!({
            "summary": "Half-way through moving auth into its own container.\nDone: types moved.\nNot done: callers.",
            "next_steps": ["update the 3 callers in api/", "re-run validate --changed"]
        }),
    );
    // Fresh agent, zero memory.
    let onb = text(&one(&root, "harna_onboarding", json!({})));
    assert!(onb.contains("moving auth into its own container"), "{onb}");
    assert!(onb.contains("update the 3 callers in api/"));
    // Resumed: the note is cleared so it never misleads a later session.
    one(&root, "harna_handoff", json!({"clear": true}));
    let later = text(&one(&root, "harna_onboarding", json!({})));
    assert!(
        !later.contains("moving auth"),
        "a stale handoff must not linger"
    );
}

/// "Actually, forget that rule — we allow unwrap in tests now."
/// The user changes their mind; the rule is retired visibly with the
/// reason, and agents stop enforcing it.
#[test]
fn a_reversed_decision_retires_the_rule_visibly() {
    let root = initialized("reverse");
    one(
        &root,
        "harna_add_rule",
        json!({
            "id": "no-unwrap", "text": "No unwrap() anywhere", "rationale": "panics"
        }),
    );
    let r = one(
        &root,
        "harna_rule_lifecycle",
        json!({
            "id": "no-unwrap", "action": "deprecate", "reason": "user: unwrap is fine in tests"
        }),
    );
    assert!(!is_error(&r), "{r}");
    let rules = std::fs::read_to_string(root.join(".harna/rules.toml")).unwrap();
    assert!(
        rules.contains("unwrap is fine in tests"),
        "the why is on record:\n{rules}"
    );
    // Briefed as visibly dead, with the reason — so an agent that remembers
    // the old rule from a stale doc is told explicitly to stop following it.
    let onb = text(&one(&root, "harna_onboarding", json!({})));
    assert!(
        onb.contains("[no-unwrap] (DEPRECATED — do not follow)"),
        "{onb}"
    );
    assert!(onb.contains("unwrap is fine in tests"));
}

/// A brand-new contributor's agent lands in a repo nobody has explained
/// to it, on a lean `core` tool profile. It still learns the vocabulary
/// and can orient, plan and check — the ritual fits in the small surface.
#[test]
fn a_new_agent_on_the_core_profile_can_still_walk_the_ritual() {
    let root = initialized("newcomer");
    let core = [("HARNA_TOOLS", "core")];
    let r = session_env(
        &root,
        &core,
        &[
            call("harna_onboarding", json!({})),
            call("harna_status", json!({})),
            call(
                "harna_place",
                json!({"idea": "retry policy for http calls"}),
            ),
            call("harna_validate", json!({})),
            call(
                "harna_handoff",
                json!({"summary": "explored; nothing changed"}),
            ),
            call("harna_roadmap", json!({"action": "list"})),
        ],
    );
    let onb = text(&r[0]);
    assert!(
        onb.contains("concepts: harna's vocabulary"),
        "vocabulary arrives once:\n{onb}"
    );
    assert!(onb.contains("parity = "));
    for step in &r[1..5] {
        assert!(!is_error(step), "ritual step failed on core: {step}");
    }
    assert!(text(&r[3]).contains("verdict: PASS"));
    // Outside the profile: not advertised, so the call is refused —
    // the agent knows to ask for `full` rather than guess.
    assert!(r[5].get("error").is_some() || is_error(&r[5]), "{}", r[5]);
}

/// A small git repo with one crate: `a.rs` exports `load`, `b.rs` calls it.
/// The map is scanned and committed, so parity has a real baseline.
/// Returns None when git is unavailable on this machine.
fn crate_repo(tag: &str) -> Option<std::path::PathBuf> {
    let root = initialized(tag);
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(&root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    if !git(&["init", "-q"]) {
        return None;
    }
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub mod a;\npub mod b;\n").unwrap();
    std::fs::write(root.join("src/a.rs"), "pub fn load() -> u32 { 1 }\n").unwrap();
    std::fs::write(
        root.join("src/b.rs"),
        "use crate::a::load;\npub fn run() -> u32 { load() }\n",
    )
    .unwrap();
    assert!(!is_error(&one(&root, "harna_scan", json!({}))));
    assert!(git(&["add", "-A"]) && git(&["commit", "-q", "-m", "baseline"]));
    Some(root)
}

/// The agent renames a public function. The human asks: "is anything else
/// affected?" — harna answers with exactly the caller that may now be
/// broken, does not ask anyone to ack the file that was edited, and does
/// not let the auto pass wave the caller through.
#[test]
fn a_breaking_rename_owes_only_its_callers() {
    let Some(root) = crate_repo("rename") else {
        return;
    };
    std::fs::write(root.join("src/a.rs"), "pub fn load_v2() -> u32 { 1 }\n").unwrap();
    let r = session(
        &root,
        &[
            call("harna_scan", json!({})),
            call("harna_parity_open", json!({"id": "rename-load"})),
            call("harna_parity_auto", json!({"id": "rename-load"})),
            call("harna_parity", json!({"open_only": true})),
        ],
    );
    let opened = text(&r[1]);
    assert!(
        opened.contains("1 open"),
        "only the caller is owed:\n{opened}"
    );
    let debts = text(&r[3]);
    assert!(debts.contains("src/b.rs"), "{debts}");
    assert!(
        debts.contains("signature removed: pub fn load() -> u32"),
        "says what may be gone:\n{debts}"
    );
    assert!(
        !debts
            .lines()
            .any(|l| l.trim_start().starts_with("! open") && l.contains(" src/a.rs ")),
        "the edited file is the origin, not a debt:\n{debts}"
    );
    assert!(
        text(&r[2]).contains("1 still open"),
        "auto does not waive a breaking caller: {}",
        text(&r[2])
    );
    // Validation blocks until a human (or the agent, on the human's word)
    // says what happened at the call site.
    assert!(text(&one(&root, "harna_validate", json!({}))).contains("verdict: BLOCKED"));

    // Human: "I updated b.rs to call load_v2."
    std::fs::write(
        root.join("src/b.rs"),
        "use crate::a::load_v2;\npub fn run() -> u32 { load_v2() }\n",
    )
    .unwrap();
    let r = session(
        &root,
        &[
            call(
                "harna_parity_resolve",
                json!({
                    "id": "rename-load", "path": "src/b.rs", "status": "ack",
                    "note": "user updated the call site to load_v2"
                }),
            ),
            call("harna_validate", json!({})),
            call("harna_parity", json!({})),
        ],
    );
    assert!(!is_error(&r[0]), "{}", r[0]);
    assert!(text(&r[1]).contains("verdict: PASS"), "{}", text(&r[1]));
    assert!(
        text(&r[2]).contains("rubber-stamp signals 0"),
        "{}",
        text(&r[2])
    );
}

/// The agent adds a helper function. Nothing anyone imports went away,
/// so the ledger owes nobody an ack — the burst commits without ceremony.
#[test]
fn an_additive_change_needs_no_acks() {
    let Some(root) = crate_repo("additive") else {
        return;
    };
    std::fs::write(
        root.join("src/a.rs"),
        "pub fn load() -> u32 { 1 }\npub fn load_twice() -> u32 { 2 }\n",
    )
    .unwrap();
    let r = session(
        &root,
        &[
            call("harna_scan", json!({})),
            call("harna_parity_open", json!({"id": "add-helper"})),
            call("harna_parity", json!({"open_only": true})),
            call("harna_validate", json!({})),
        ],
    );
    assert!(
        text(&r[1]).contains("0 open — nothing owed"),
        "{}",
        text(&r[1])
    );
    assert_eq!(text(&r[2]), "no open parity debts");
    assert!(text(&r[3]).contains("verdict: PASS"));
}

/// "Add a retry policy to the http layer." The agent scaffolds it — and
/// the crate builds right away: the module is declared up its folder
/// chain, registered in the manifest with its test, nothing hand-wired.
#[test]
fn a_scaffolded_component_compiles_without_hand_wiring() {
    let Some(root) = crate_repo("scaffold-build") else {
        return;
    };
    let r = session(
        &root,
        &[call(
            "harna_scaffold",
            json!({
                "layer": "net", "container": "http", "name": "retry-policy",
                "kind": "primitive", "description": "backoff for transient http failures"
            }),
        )],
    );
    let msg = text(&r[0]);
    assert!(msg.contains("src/net/http/retry_policy.rs"), "{msg}");
    assert!(msg.contains("wired: "), "{msg}");
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    assert!(lib.contains("pub mod net;"), "{lib}");

    // The proof: it compiles (skipped where cargo is unavailable).
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    if let Ok(out) = std::process::Command::new(&cargo)
        .args(["check", "--quiet", "--offline"])
        .env("CARGO_TARGET_DIR", root.join("target"))
        .current_dir(&root)
        .output()
    {
        assert!(
            out.status.success(),
            "scaffolded crate must build:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let manifest = std::fs::read_to_string(root.join(".harna/manifest.toml")).unwrap();
    assert!(
        manifest.contains("tests = [\"src/net/http/retry_policy.rs\"]"),
        "{manifest}"
    );
    assert!(text(&one(&root, "harna_validate", json!({}))).contains("verdict: PASS"));
}

/// The agent keeps adding `*_data.rs` files beside each other. harna names
/// the folder that was never created; the agent fixes it with `harna_move`
/// — imports, mod tree and manifest follow — and the crate still builds.
#[test]
fn a_name_encoded_folder_is_fixed_with_one_move_each() {
    let Some(root) = crate_repo("cluster") else {
        return;
    };
    for name in ["terms", "lessons", "roadmap"] {
        std::fs::write(
            root.join(format!("src/{name}_data.rs")),
            format!("pub struct {}Data;\n", name.to_uppercase()),
        )
        .unwrap();
    }
    std::fs::write(
        root.join("src/lib.rs"),
        "pub mod a;\npub mod b;\npub mod terms_data;\npub mod lessons_data;\npub mod roadmap_data;\n",
    )
    .unwrap();
    std::fs::write(root.join("src/b.rs"), "use crate::a::load;\nuse crate::terms_data::TERMSData;\npub fn run() -> u32 { let _ = TERMSData; load() }\n").unwrap();
    let before = text(
        &session(
            &root,
            &[
                call("harna_scan", json!({})),
                call("harna_validate", json!({})),
            ],
        )[1],
    );
    assert!(
        before.contains("3 files share the name token `data`"),
        "{before}"
    );

    let moves: Vec<_> = ["terms", "lessons", "roadmap"]
        .iter()
        .map(|n| {
            call(
                "harna_move",
                json!({"from": format!("src/{n}_data.rs"), "to": format!("src/data/{n}.rs")}),
            )
        })
        .collect();
    let r = session(&root, &moves);
    assert!(
        text(&r[0]).contains("rewritten in 1 file(s)"),
        "the caller follows: {}",
        text(&r[0])
    );
    let b = std::fs::read_to_string(root.join("src/b.rs")).unwrap();
    assert!(b.contains("use crate::data::terms::TERMSData;"), "{b}");
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("pub mod data;") && !lib.contains("_data;"),
        "{lib}"
    );

    let after = text(
        &session(
            &root,
            &[
                call("harna_scan", json!({})),
                call("harna_validate", json!({})),
            ],
        )[1],
    );
    assert!(!after.contains("name-cluster"), "{after}");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    if let Ok(out) = std::process::Command::new(&cargo)
        .args(["check", "--quiet", "--offline"])
        .env("CARGO_TARGET_DIR", root.join("target"))
        .current_dir(&root)
        .output()
    {
        assert!(
            out.status.success(),
            "restructured crate must build:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// The user asks the agent to "just mark that warning as fine". An Info
/// can be accepted with a reason; an Error cannot — however it is asked.
#[test]
fn accepting_a_warning_never_silences_an_error() {
    let root = initialized("accept");
    let r = session(
        &root,
        &[
            call(
                "harna_acknowledge",
                json!({
                    "action": "add", "check": "hotspot", "file": "src/big.rs",
                    "reason": "user: splitting it is scheduled for 0.3.37"
                }),
            ),
            call("harna_acknowledge", json!({"action": "list"})),
            call(
                "harna_acknowledge",
                json!({"action": "add", "check": "hotspot", "file": "src/big.rs"}),
            ),
        ],
    );
    assert!(text(&r[1]).contains("scheduled for 0.3.37"));
    assert!(is_error(&r[2]), "no reason, no acceptance: {}", r[2]);
}

/// "The catalog is really the index now — rename it. And the UI talks to
/// the backend." (font-manager, 0.3.42): the agent hand-edited the
/// manifest for both; now each is one typed call, checked before saving.
#[test]
fn a_container_is_renamed_and_a_layer_dependency_declared_without_hand_edits() {
    let root = initialized("manifest-edit");
    let r = session(
        &root,
        &[
            call(
                "harna_scaffold",
                json!({"layer": "backend", "container": "catalog", "name": "families", "kind": "primitive"}),
            ),
            call(
                "harna_scaffold",
                json!({"layer": "ui", "container": "app", "name": "window", "kind": "primitive"}),
            ),
            call(
                "harna_manifest_edit",
                json!({"action": "rename", "name": "catalog", "new_name": "index"}),
            ),
            call(
                "harna_manifest_edit",
                json!({"action": "layer", "name": "ui", "depends_on": "backend"}),
            ),
            call(
                "harna_manifest_edit",
                json!({"action": "layer", "name": "ui", "depends_on": "frontend"}),
            ),
        ],
    );
    assert!(
        text(&r[2]).contains("`catalog` -> `index`"),
        "{}",
        text(&r[2])
    );
    assert!(
        text(&r[3]).contains("depends_on [] -> [backend]"),
        "{}",
        text(&r[3])
    );
    assert!(is_error(&r[4]), "unknown layer refused: {}", text(&r[4]));
    let manifest = std::fs::read_to_string(root.join(".harna/manifest.toml")).unwrap();
    assert!(manifest.contains("name = \"index\""), "{manifest}");
    assert!(
        manifest.contains("path = \"src/backend/catalog\""),
        "born with its path"
    );
    assert!(!manifest.contains("frontend"), "a refusal writes nothing");
}
