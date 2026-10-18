//! The MCP resources (`harna://…`) and the server handler wiring.

use super::*;

#[tool_handler]
impl ServerHandler for HarnaTools {
    fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<
        Output = Result<rmcp::model::ListResourcesResult, rmcp::ErrorData>,
    > + Send + '_ {
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
        std::future::ready(Ok(rmcp::model::ListResourcesResult::with_all_items(resources)))
    }

    fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<
        Output = Result<rmcp::model::ReadResourceResult, rmcp::ErrorData>,
    > + Send + '_ {
        let file = match request.uri.as_str() {
            "harna://manifest.toml" => Some("manifest.toml"),
            "harna://rules.toml" => Some("rules.toml"),
            "harna://terms.toml" => Some("terms.toml"),
            "harna://non-goals.toml" => Some("non-goals.toml"),
            "harna://lessons.toml" => Some("lessons.toml"),
            "harna://roadmap.toml" => Some("roadmap.toml"),
            "harna://digest.json" => Some("digest.json"),
            _ => None,
        };
        let result = match file {
            Some(f) => {
                let path = self.root.join(".harna").join(f);
                std::fs::read_to_string(&path).map(|text| {
                    rmcp::model::ReadResourceResult {
                        contents: vec![rmcp::model::ResourceContents::text(text, request.uri.clone())],
                    }
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
