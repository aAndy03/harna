//! The MCP resources (`harna://…`) and the server handler wiring.

use super::*;

#[tool_handler]
impl ServerHandler for HarnaTools {
    /// What the server offers, said at `initialize` (0.9.2, field test: it
    /// advertised no capabilities and named itself `rmcp`).
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            protocol_version: Default::default(),
            capabilities: rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            server_info: rmcp::model::Implementation {
                name: "harna".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions: Some(
                "harna keeps this repository's architecture as data in .harna/. Start with harna_onboarding; validate with harna_validate (changed_only after edits)."
                    .into(),
            ),
        }
    }

    fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::ListResourcesResult, rmcp::ErrorData>>
           + Send
           + '_ {
        let resources = vec![
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://manifest.toml".into(),
                    name: "manifest".into(),
                    description: Some(
                        "architecture: layers -> containers -> components (read-only)"
                            .into(),
                    ),
                    mime_type: Some("application/toml".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://rules.toml".into(),
                    name: "rules".into(),
                    description: Some(
                        "coding rules with rationale, scope, origin (read-only)".into(),
                    ),
                    mime_type: Some("application/toml".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://terms.toml".into(),
                    name: "terms".into(),
                    description: Some(
                        "glossary: agent vocabulary -> harna vocabulary, with meanings (read-only)"
                            .into(),
                    ),
                    mime_type: Some("application/toml".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://non-goals.toml".into(),
                    name: "non-goals".into(),
                    description: Some(
                        "deliberate product refusals the agent must not propose around (read-only)"
                            .into(),
                    ),
                    mime_type: Some("application/toml".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://lessons.toml".into(),
                    name: "lessons".into(),
                    description: Some(
                        "recorded dead ends: tried-and-failed with verdicts and evidence (read-only)"
                            .into(),
                    ),
                    mime_type: Some("application/toml".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://roadmap.toml".into(),
                    name: "roadmap".into(),
                    description: Some(
                        "declared milestones with states — the project's path, never generated (read-only)"
                            .into(),
                    ),
                    mime_type: Some("application/toml".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://qualities.toml".into(),
                    name: "qualities".into(),
                    description: Some(
                        "long-term intent: what the software must hold (scale, latency, offline, data loss, privacy) (read-only)"
                            .into(),
                    ),
                    mime_type: Some("application/toml".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://services.toml".into(),
                    name: "services".into(),
                    description: Some(
                        "services: the containers each runs alone and the resources each alone writes (read-only)".into(),
                    ),
                    mime_type: Some("application/toml".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://tools".into(),
                    name: "tools".into(),
                    description: Some(
                        "harna's toolbox: what each tool does, when to reach for it, how (all optional)"
                            .into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://digest.json".into(),
                    name: "digest".into(),
                    description: Some(
                        "compressed code map: signatures, inventory, deps (read-only)"
                            .into(),
                    ),
                    mime_type: Some("application/json".into()),
                    size: None,
                },
                annotations: None,
            },
        ];
        std::future::ready(Ok(rmcp::model::ListResourcesResult::with_all_items(
            resources,
        )))
    }

    fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::ReadResourceResult, rmcp::ErrorData>>
           + Send
           + '_ {
        // The toolbox is harna's own data, not a repo file (0.3.50).
        if request.uri == "harna://tools" {
            return std::future::ready(Ok(rmcp::model::ReadResourceResult {
                contents: vec![rmcp::model::ResourceContents::text(
                    harna_core::composition::briefing::toolbox::render_catalog(),
                    request.uri.clone(),
                )],
            }));
        }
        let file = match request.uri.as_str() {
            "harna://manifest.toml" => Some("manifest.toml"),
            "harna://rules.toml" => Some("rules.toml"),
            "harna://terms.toml" => Some("terms.toml"),
            "harna://non-goals.toml" => Some("non-goals.toml"),
            "harna://lessons.toml" => Some("lessons.toml"),
            "harna://roadmap.toml" => Some("roadmap.toml"),
            "harna://qualities.toml" => Some("qualities.toml"),
            "harna://services.toml" => Some("services.toml"),
            "harna://digest.json" => Some("digest.json"),
            _ => None,
        };
        let result = match file {
            Some(f) => {
                let path = self.root.join(".harna").join(f);
                std::fs::read_to_string(&path).map(|text| rmcp::model::ReadResourceResult {
                    contents: vec![rmcp::model::ResourceContents::text(
                        text,
                        request.uri.clone(),
                    )],
                })
            }
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("unknown harna resource: {}", request.uri),
            )),
        };
        std::future::ready(result.map_err(|e| rmcp::ErrorData {
            code: rmcp::model::ErrorCode::INVALID_PARAMS,
            message: e.to_string().into(),
            data: None,
        }))
    }
}
