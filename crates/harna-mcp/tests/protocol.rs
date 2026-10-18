//! End-to-end over stdio: every tool advertises an object input schema
//! (callable by any MCP client), tool failures are `isError` results
//! rather than JSON-RPC errors, and `--version` answers without serving.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("harna-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Run one MCP session: initialize, then each request in order; returns
/// the response to each request (by id).
fn session(root: &PathBuf, requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_harna-mcp"))
        .env("HARNA_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn harna-mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut send = |v: Value| writeln!(stdin, "{v}").unwrap();
    let mut recv = |id: i64| -> Value {
        loop {
            let mut line = String::new();
            assert!(out.read_line(&mut line).unwrap() > 0, "server closed before id {id}");
            let v: Value = serde_json::from_str(&line).expect("stdout is pure JSON-RPC");
            if v["id"] == id {
                return v;
            }
        }
    };
    send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
        "protocolVersion":"2024-11-05","capabilities":{},
        "clientInfo":{"name":"test","version":"0"}}}));
    recv(0);
    send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let mut responses = Vec::new();
    for (i, r) in requests.iter().enumerate() {
        let id = i as i64 + 1;
        let mut r = r.clone();
        r["jsonrpc"] = json!("2.0");
        r["id"] = json!(id);
        send(r);
        responses.push(recv(id));
    }
    drop(stdin);
    let _ = child.wait();
    responses
}

#[test]
fn every_tool_schema_is_an_object_and_migrate_restore_are_callable() {
    let root = scratch("schemas");
    let r = session(
        &root,
        &[
            json!({"method":"tools/list","params":{}}),
            json!({"method":"tools/call","params":{"name":"harna_migrate","arguments":{"dry":true}}}),
            json!({"method":"tools/call","params":{"name":"harna_restore","arguments":{"name":"rules.toml"}}}),
        ],
    );
    let tools = r[0]["result"]["tools"].as_array().expect("tool list");
    assert!(tools.len() > 20);
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object", "tool {} schema: {}", t["name"], t["inputSchema"]);
    }
    // Arguments now deserialize: no -32602 invalid params.
    assert!(r[1].get("error").is_none(), "migrate: {}", r[1]);
    assert!(r[2].get("error").is_none(), "restore: {}", r[2]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tool_failures_are_is_error_results_not_protocol_errors() {
    let root = scratch("iserror");
    // Uninitialized root: restore fails inside the tool.
    let r = session(
        &root,
        &[json!({"method":"tools/call","params":{"name":"harna_restore","arguments":{"name":"rules.toml"}}})],
    );
    assert!(r[0].get("error").is_none(), "must not be a JSON-RPC error: {}", r[0]);
    assert_eq!(r[0]["result"]["isError"], true, "{}", r[0]);
    let text = r[0]["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("rules.toml"), "the model sees the message: {text}");
    let _ = std::fs::remove_dir_all(&root);
}

fn call(name: &str, arguments: Value) -> Value {
    json!({"method":"tools/call","params":{"name":name,"arguments":arguments}})
}

fn text(r: &Value) -> String {
    assert!(r.get("error").is_none(), "protocol error: {r}");
    r["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn cli_surface_is_reachable_over_mcp() {
    let root = scratch("surface");
    // An initialized repo: the CLI is not a dependency here, so seed .harna/.
    std::fs::create_dir_all(root.join(".harna")).unwrap();
    std::fs::write(root.join(".harna/manifest.toml"), "schema_version = 1\n").unwrap();
    std::fs::write(root.join(".harna/rules.toml"), "schema_version = 1\n").unwrap();
    std::fs::write(root.join("big.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
    let r = session(
        &root,
        &[
            json!({"method":"tools/list","params":{}}),
            call("harna_validate", json!({})),
            call("harna_roadmap", json!({"action":"add","id":"m1","title":"first"})),
            call("harna_roadmap", json!({"action":"set","id":"m1","state":"current"})),
            call("harna_roadmap", json!({"action":"list"})),
            call("harna_roadmap", json!({"action":"set","id":"m1","state":"bogus"})),
            call("harna_acknowledge", json!({"action":"add","check":"hotspot","file":"big.rs","reason":"tracked"})),
            call("harna_acknowledge", json!({"action":"list"})),
            call("harna_non_goal", json!({"action":"add","id":"ng","refusal":"no cloud","reason":"local-first"})),
            call("harna_lesson", json!({"action":"add","id":"l1","claim":"tried x","verdict":"too slow"})),
            call("harna_terms", json!({"action":"add","word":"engine","means":"a layer that computes"})),
            call("harna_decompose", json!({"file":"../outside.rs"})),
            call("harna_decompose", json!({"file":"big.rs"})),
            call("harna_add_rule", json!({"id":"r1","text":"t","rationale":"r"})),
            call("harna_status", json!({})),
        ],
    );
    let names: Vec<&str> = r[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for want in [
        "harna_status", "harna_preflight", "harna_validate_intent", "harna_acknowledge",
        "harna_decompose", "harna_non_goal", "harna_lesson", "harna_roadmap", "harna_terms",
        "harna_adopt", "harna_adapt",
    ] {
        assert!(names.contains(&want), "missing {want}: {names:?}");
    }
    assert!(text(&r[1]).contains("verdict: PASS"), "validate states its verdict: {}", text(&r[1]));
    assert!(text(&r[4]).contains("[current] m1 = first"), "{}", text(&r[4]));
    assert_eq!(r[5]["result"]["isError"], true, "bad state refused");
    assert!(text(&r[7]).contains("[hotspot] big.rs"), "{}", text(&r[7]));
    assert!(text(&r[8]).contains("refusal `ng` in force"));
    assert!(text(&r[9]).contains("lesson `l1` recorded"));
    assert!(text(&r[10]).contains("term `engine` recorded"));
    assert_eq!(r[11]["result"]["isError"], true, "decompose stays inside the repo");
    assert!(r[12]["result"]["isError"] != true, "{}", r[12]);
    // Rule origins carry the same ISO date the CLI writes.
    let rules = std::fs::read_to_string(root.join(".harna/rules.toml")).unwrap();
    let origin = rules.lines().find(|l| l.starts_with("origin")).expect("origin line");
    let date = origin.trim_end_matches('"').rsplit(", ").next().unwrap();
    assert!(
        date.len() == 10 && date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-',
        "ISO date, not unix-days: {origin}"
    );
    assert!(r[14]["result"]["isError"] != true, "{}", r[14]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn version_flag_answers_without_serving() {
    let out = Command::new(env!("CARGO_BIN_EXE_harna-mcp"))
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("harna-mcp "));
}
