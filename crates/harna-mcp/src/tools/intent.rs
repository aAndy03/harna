//! MCP tool for the owner's intent (1.1.2): the vision kept as written and
//! the assumptions the agent made on the way — one tool, `action`-led, the
//! same core the CLI's `harna assume` and `harna vision` use.

use std::future::Future;

use rmcp::{handler::server::tool::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{required, today, HarnaTools};
use harna_core::schema::registers::assumptions::{self, Assumption};

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct AssumeRequest {
    /// add | confirm | correct | list | next | vision (keep text) | show-vision.
    pub action: String,
    /// Stable id (add, confirm, correct).
    pub id: Option<String>,
    /// What was assumed (add).
    pub statement: Option<String>,
    /// The question that settles it (add).
    pub question: Option<String>,
    /// What the code does meanwhile (add).
    pub default: Option<String>,
    /// What the answer would change (add).
    pub impact: Option<String>,
    /// Roadmap milestone it is due by (add).
    pub by: Option<String>,
    /// Holds the release until answered (add).
    pub blocking: Option<bool>,
    /// The owner's answer (confirm, correct) or the vision text (vision).
    pub answer: Option<String>,
}

#[tool_router(router = intent_router, vis = "pub(super)")]
impl HarnaTools {
    #[tool(
        description = "A prompt is a vision: record each guess (add), hand the owner the next questions (next), settle (confirm/correct); vision keeps the prompt."
    )]
    async fn harna_assume(
        &self,
        Parameters(AssumeRequest {
            action,
            id,
            statement,
            question,
            default,
            impact,
            by,
            blocking,
            answer,
        }): Parameters<AssumeRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut reg = store.load_assumptions()?;
            match action.as_str() {
                "add" => {
                    let id = required(id, "id", "add")?;
                    let by = by.unwrap_or_default();
                    if !by.is_empty() && store.load_roadmap()?.find(&by).is_none() {
                        anyhow::bail!("no milestone `{by}` on the roadmap");
                    }
                    reg.upsert(Assumption {
                        id: id.clone(),
                        statement: required(statement, "statement", "add")?,
                        question: required(question, "question", "add")?,
                        default: default.unwrap_or_default(),
                        impact: impact.unwrap_or_default(),
                        by,
                        blocking: blocking.unwrap_or(false),
                        state: "open".into(),
                        answer: String::new(),
                        recorded: today(),
                    });
                    store.save_assumptions(&reg)?;
                    Ok(format!("assumption `{id}` recorded ({} open)", reg.open().len()))
                }
                "confirm" | "correct" => {
                    let state = if action == "confirm" { "confirmed" } else { "corrected" };
                    let line = reg
                        .settle(
                            &required(id, "id", &action)?,
                            state,
                            &answer.unwrap_or_default(),
                        )?
                        .line();
                    store.save_assumptions(&reg)?;
                    Ok(line)
                }
                "list" if reg.is_empty() => Ok("no assumptions recorded".into()),
                "list" => Ok(reg
                    .assumptions
                    .iter()
                    .map(Assumption::line)
                    .collect::<Vec<_>>()
                    .join("\n")),
                "next" => {
                    let order: Vec<String> = store
                        .load_roadmap()?
                        .milestones
                        .iter()
                        .map(|m| m.id.clone())
                        .collect();
                    Ok(assumptions::ask(&reg.next(3, &order)))
                }
                "vision" => {
                    let text = required(answer, "answer (the vision text)", "vision")?;
                    store.save_vision(&text)?;
                    Ok("vision kept in .harna/vision.md".into())
                }
                "show-vision" => Ok(store
                    .load_vision()
                    .unwrap_or_else(|| "no vision kept".into())),
                other => anyhow::bail!(
                    "unknown action `{other}` — add | confirm | correct | list | next | vision | show-vision"
                ),
            }
        })
    }
}
