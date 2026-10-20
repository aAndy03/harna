//! Shared driver for the MCP integration tests: spawn the real
//! `harna-mcp` binary over stdio, initialize, send requests, collect
//! responses. One `session` = one agent process (a fresh context).

#![allow(dead_code, reason = "each test binary uses its own subset")]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

/// A fresh scratch directory per test.
pub fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("harna-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A scratch repo with `.harna/` initialized (manifest + rules starters).
pub fn initialized(tag: &str) -> PathBuf {
    let root = scratch(tag);
    std::fs::create_dir_all(root.join(".harna")).unwrap();
    std::fs::write(root.join(".harna/manifest.toml"), "schema_version = 1\n").unwrap();
    std::fs::write(root.join(".harna/rules.toml"), "schema_version = 1\n").unwrap();
    root
}

/// Run one MCP session: initialize, then each request in order; returns
/// the response to each request.
pub fn session(root: &PathBuf, requests: &[Value]) -> Vec<Value> {
    session_env(root, &[], requests)
}

pub fn session_env(root: &PathBuf, envs: &[(&str, &str)], requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_harna-mcp"))
        .env("HARNA_ROOT", root)
        .envs(envs.iter().copied())
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
            assert!(
                out.read_line(&mut line).unwrap() > 0,
                "server closed before id {id}"
            );
            let v: Value = serde_json::from_str(&line).expect("stdout is pure JSON-RPC");
            if v["id"] == id {
                return v;
            }
        }
    };
    send(
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
        "protocolVersion":"2024-11-05","capabilities":{},
        "clientInfo":{"name":"test","version":"0"}}}),
    );
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

/// A `tools/call` request.
pub fn call(name: &str, arguments: Value) -> Value {
    json!({"method":"tools/call","params":{"name":name,"arguments":arguments}})
}

/// The text of a successful tool result (panics on protocol errors).
pub fn text(r: &Value) -> String {
    assert!(r.get("error").is_none(), "protocol error: {r}");
    r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// One call in its own session — a step in a multi-session story.
pub fn one(root: &PathBuf, name: &str, arguments: Value) -> Value {
    session(root, &[call(name, arguments)]).remove(0)
}

pub fn is_error(r: &Value) -> bool {
    r["result"]["isError"] == true
}

/// The server's answer to `initialize` (0.9.2).
pub fn initialize_result(root: &PathBuf) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_harna-mcp"))
        .env("HARNA_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn harna-mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"test","version":"0"}}})
    )
    .unwrap();
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    drop(stdin);
    let _ = child.wait();
    serde_json::from_str(&line).expect("an initialize response")
}
