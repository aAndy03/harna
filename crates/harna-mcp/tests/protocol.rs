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
