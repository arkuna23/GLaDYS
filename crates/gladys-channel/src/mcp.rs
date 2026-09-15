use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::Deserialize;
use serde_json::Value;

use crate::error::ChannelError;
use crate::message::{Conversation, Part};
use crate::service::{
    CallParams, CapabilitiesParams, ConversationsParams, GetParams, HistoryParams, ReactParams,
    RecallParams, SearchParams, SendParams, Service,
};

#[derive(Clone)]
pub struct ChannelMcp {
    service: Arc<Service>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl ChannelMcp {
    pub fn new(service: Arc<Service>) -> Self {
        Self {
            service,
            tool_router: Self::tool_router(),
        }
    }

    fn wrap<T: serde::Serialize>(result: Result<T, ChannelError>) -> Result<CallToolResult, rmcp::ErrorData> {
        match result {
            Ok(v) => {
                let value = serde_json::to_value(v).unwrap_or(Value::Null);
                let value = match value {
                    Value::Object(_) => value,
                    other => serde_json::json!({ "data": other }),
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
struct ListArgs {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CapsArgs {
    #[serde(default)]
    account: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SendArgs {
    account: String,
    conversation: Conversation,
    parts: Vec<Part>,
    #[serde(default)]
    idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetArgs {
    account: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    platform_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HistoryArgs {
    account: String,
    conversation: Conversation,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    before_ts: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RecallArgs {
    account: String,
    platform_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReactArgs {
    account: String,
    platform_id: String,
    emoji: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ConversationsArgs {
    #[serde(default)]
    account: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CallArgs {
    account: String,
    op: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    idempotency_key: Option<String>,
}

#[tool_router]
impl ChannelMcp {
    #[tool(description = "List channel accounts and connection status")]
    async fn channel_list(&self, Parameters(_args): Parameters<ListArgs>) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(Ok(self.service.list_accounts()))
    }

    #[tool(description = "Discover common and native capabilities for an account")]
    async fn channel_capabilities(
        &self,
        Parameters(args): Parameters<CapsArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.service.capabilities(CapabilitiesParams {
            account: args.account,
        }))
    }

    #[tool(description = "Send a message using common parts")]
    async fn channel_send(
        &self,
        Parameters(args): Parameters<SendArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(
            self.service
                .send(SendParams {
                    account: args.account,
                    conversation: args.conversation,
                    parts: args.parts,
                    idempotency_key: args.idempotency_key,
                })
                .await,
        )
    }

    #[tool(description = "Get one stored message by id or platform_id")]
    async fn channel_get(
        &self,
        Parameters(args): Parameters<GetArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(
            self.service
                .get(GetParams {
                    account: args.account,
                    id: args.id,
                    platform_id: args.platform_id,
                })
                .await,
        )
    }

    #[tool(description = "Recent messages in a conversation from local store")]
    async fn channel_history(
        &self,
        Parameters(args): Parameters<HistoryArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.service.history(HistoryParams {
            account: args.account,
            conversation: args.conversation,
            limit: args.limit.unwrap_or(50),
            before_ts: args.before_ts,
        }))
    }

    #[tool(description = "Full-text search over stored messages")]
    async fn channel_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.service.search(SearchParams {
            query: args.query,
            account: args.account,
            limit: args.limit.unwrap_or(50),
        }))
    }

    #[tool(description = "Recall / delete a sent message")]
    async fn channel_recall(
        &self,
        Parameters(args): Parameters<RecallArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(
            self.service
                .recall(RecallParams {
                    account: args.account,
                    platform_id: args.platform_id,
                })
                .await,
        )
    }

    #[tool(description = "React to a message (napcat profile)")]
    async fn channel_react(
        &self,
        Parameters(args): Parameters<ReactArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(
            self.service
                .react(ReactParams {
                    account: args.account,
                    platform_id: args.platform_id,
                    emoji: args.emoji,
                })
                .await
                .map(|_| json_ok()),
        )
    }

    #[tool(description = "List known conversations")]
    async fn channel_conversations(
        &self,
        Parameters(args): Parameters<ConversationsArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.service.conversations(ConversationsParams {
            account: args.account,
        }))
    }

    #[tool(description = "Call a platform-native operation advertised by channel_capabilities")]
    async fn channel_call(
        &self,
        Parameters(args): Parameters<CallArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(
            self.service
                .call(CallParams {
                    account: args.account,
                    op: args.op,
                    params: args.params,
                    idempotency_key: args.idempotency_key,
                })
                .await,
        )
    }
}

fn json_ok() -> Value {
    serde_json::json!({"ok": true})
}

#[tool_handler]
impl ServerHandler for ChannelMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "GLaDYS channel tools. Use channel_capabilities then channel_call for native ops.",
            )
    }
}
