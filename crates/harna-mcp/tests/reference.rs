//! The MCP reference (0.9.3), rendered from what the server itself lists:
//! every tool with its parameters, every resource. `docs/reference/mcp.md`
//! must match; `HARNA_BLESS=1 cargo test -p harna-mcp --test reference`
//! rewrites it.

mod common;

use common::{initialized, session};
use serde_json::{json, Value};

fn page(tools: &[Value], resources: &[Value]) -> String {
    let mut out = format!(
        "# MCP reference\n\nGenerated from the server's own `tools/list` and `resources/list` (`harna-mcp`) — do not edit.\n\n\
         {} tools and {} resources. Wire the server with one config line (`harna-mcp`, run in the repository); \
         `HARNA_TOOLS=core` lists the core set only.\n\n## Tools\n",
        tools.len(),
        resources.len()
    );
    for t in tools {
        out.push_str(&format!(
            "\n### `{}`\n\n{}\n",
            t["name"].as_str().unwrap_or(""),
            t["description"].as_str().unwrap_or("")
        ));
        let props = t["inputSchema"]["properties"].as_object();
        let required: Vec<&str> = t["inputSchema"]["required"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        if let Some(props) = props.filter(|p| !p.is_empty()) {
            out.push_str("\n| Parameter | Type | Meaning |\n|---|---|---|\n");
            for (name, p) in props {
                let ty = p["type"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| {
                        p["type"].as_array().map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(" | ")
                        })
                    })
                    .unwrap_or_default();
                out.push_str(&format!(
                    "| `{name}`{} | {ty} | {} |\n",
                    if required.contains(&name.as_str()) {
                        " (required)"
                    } else {
                        ""
                    },
                    p["description"].as_str().unwrap_or("").replace('|', r"\|")
                ));
            }
        }
    }
    out.push_str("\n## Resources\n\n| URI | What it holds |\n|---|---|\n");
    for r in resources {
        out.push_str(&format!(
            "| `{}` | {} |\n",
            r["uri"].as_str().unwrap_or(""),
            r["description"].as_str().unwrap_or("").replace('|', r"\|")
        ));
    }
    out
}

#[test]
fn the_mcp_reference_matches_the_server() {
    let root = initialized("reference");
    let r = session(
        &root,
        &[
            json!({"method":"tools/list","params":{}}),
            json!({"method":"resources/list","params":{}}),
        ],
    );
    // sorted: the page must not depend on the router's listing order
    let mut tools = r[0]["result"]["tools"].as_array().expect("tools").clone();
    tools.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let mut resources = r[1]["result"]["resources"]
        .as_array()
        .expect("resources")
        .clone();
    resources.sort_by(|a, b| a["uri"].as_str().cmp(&b["uri"].as_str()));
    let text = page(&tools, &resources);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/mcp.md");
    if std::env::var_os("HARNA_BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        on_disk == text,
        "docs/reference/mcp.md is stale — run `HARNA_BLESS=1 cargo test -p harna-mcp --test reference` and commit"
    );
}
