use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::client::GatewayClient;

#[derive(Clone)]
pub struct SchedulerMcp {
    client: Arc<GatewayClient>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl SchedulerMcp {
    pub fn new(client: Arc<GatewayClient>) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }

    async fn wrap(result: crate::error::Result<Value>) -> Result<CallToolResult, rmcp::ErrorData> {
        match result {
            Ok(v) => {
                let value = match v {
                    Value::Object(_) => v,
                    other => json!({ "data": other }),
                };
                Ok(CallToolResult::structured(value))
            }
            Err(e) => Ok(CallToolResult::error(vec![rmcp::model::ContentBlock::text(
                e.to_string(),
            )])),
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct Conversation {
    kind: String,
    peer: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CronArgs {
    account: String,
    channel: String,
    conversation: Conversation,
    text: String,
    cron: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DelayArgs {
    account: String,
    channel: String,
    conversation: Conversation,
    text: String,
    after_secs: u64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct StdioArgs {
    account: String,
    channel: String,
    conversation: Conversation,
    #[serde(default)]
    text: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct IdArgs {
    id: String,
}

fn conv(c: Conversation) -> Value {
    json!({ "kind": c.kind, "peer": c.peer })
}

#[tool_router]
impl SchedulerMcp {
    #[tool(description = "Create a repeating cron job (6-field UTC: sec min hour day month dow)")]
    async fn schedule_cron(
        &self,
        Parameters(args): Parameters<CronArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(
            self.client
                .create(json!({
                    "kind": "cron",
                    "account": args.account,
                    "channel": args.channel,
                    "conversation": conv(args.conversation),
                    "text": args.text,
                    "cron": args.cron,
                }))
                .await,
        )
        .await
    }

    #[tool(description = "Create a one-shot delay job; fires after after_secs then deletes")]
    async fn schedule_delay(
        &self,
        Parameters(args): Parameters<DelayArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(
            self.client
                .create(json!({
                    "kind": "delay",
                    "account": args.account,
                    "channel": args.channel,
                    "conversation": conv(args.conversation),
                    "text": args.text,
                    "after_secs": args.after_secs,
                }))
                .await,
        )
        .await
    }

    #[tool(description = "Spawn a command; each non-empty stdout line fires a scheduler trigger")]
    async fn schedule_stdio(
        &self,
        Parameters(args): Parameters<StdioArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(
            self.client
                .create(json!({
                    "kind": "stdio",
                    "account": args.account,
                    "channel": args.channel,
                    "conversation": conv(args.conversation),
                    "text": args.text,
                    "command": args.command,
                    "args": args.args,
                }))
                .await,
        )
        .await
    }

    #[tool(description = "List scheduler jobs")]
    async fn schedule_list(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.client.list().await).await
    }

    #[tool(description = "Cancel a scheduler job by id")]
    async fn schedule_cancel(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.client.cancel(&args.id).await).await
    }
}

#[tool_handler]
impl ServerHandler for SchedulerMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("GLaDYS scheduler. Jobs persist on Gateway; this MCP only creates, lists, and cancels.")
    }
}

