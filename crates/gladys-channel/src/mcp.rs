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

fn need<T>(v: Option<T>, name: &str) -> Result<T, ChannelError> {
    v.ok_or_else(|| ChannelError::Invalid(format!("{name} required")))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ChannelArgs {
    /// list | capabilities | blob_upload_url | send | get | history | search | recall | react | conversations
    op: String,
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    conversation: Option<Conversation>,
    #[serde(default)]
    parts: Option<Vec<Part>>,
    #[serde(default)]
    reply: Option<String>,
    #[serde(default)]
    idempotency_key: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    platform_id: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    before: Option<u32>,
    #[serde(default)]
    before_ts: Option<i64>,
    #[serde(default)]
    emoji: Option<String>,
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
    #[tool(
        description = "Common channel ops. op=list|capabilities|blob_upload_url|send|get|history|search|recall|react|conversations. send: account+conversation+parts (text/mention/image/audio/video/file). Never put CQ codes in text; mention and media are separate parts. reply quotes a platform id. Media: blob_upload_url, PUT the file, send blob_id. get/history/search return compact groups (CQ text)."
    )]
    async fn channel(
        &self,
        Parameters(args): Parameters<ChannelArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.dispatch(args).await)
    }

    #[tool(description = "Call a platform-native operation advertised by channel op=capabilities")]
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

impl ChannelMcp {
    async fn dispatch(&self, args: ChannelArgs) -> Result<Value, ChannelError> {
        match args.op.as_str() {
            "list" => serde_json::to_value(self.service.list_accounts()).map_err(Into::into),
            "capabilities" => serde_json::to_value(self.service.capabilities(CapabilitiesParams {
                account: args.account,
            })?)
            .map_err(Into::into),
            "blob_upload_url" => {
                serde_json::to_value(self.service.blob_upload_slot().await).map_err(Into::into)
            }
            "send" => {
                let ack = self
                    .service
                    .send(SendParams {
                        account: need(args.account, "account")?,
                        conversation: need(args.conversation, "conversation")?,
                        parts: need(args.parts, "parts")?,
                        reply: args.reply,
                        idempotency_key: args.idempotency_key,
                    })
                    .await?;
                serde_json::to_value(ack).map_err(Into::into)
            }
            "get" => {
                let group = self
                    .service
                    .get(GetParams {
                        account: need(args.account, "account")?,
                        id: args.id,
                        platform_id: args.platform_id,
                    })
                    .await?;
                serde_json::to_value(group).map_err(Into::into)
            }
            "history" => serde_json::to_value(self.service.history(HistoryParams {
                account: need(args.account, "account")?,
                conversation: need(args.conversation, "conversation")?,
                limit: args.limit.unwrap_or(50),
                before_ts: args.before_ts,
            })?)
            .map_err(Into::into),
            "search" => serde_json::to_value(self.service.search(SearchParams {
                query: need(args.query, "query")?,
                account: args.account,
                limit: args.limit.unwrap_or(50),
                before: args.before.unwrap_or(10),
            })?)
            .map_err(Into::into),
            "recall" => {
                let v = self
                    .service
                    .recall(RecallParams {
                        account: need(args.account, "account")?,
                        platform_id: need(args.platform_id, "platform_id")?,
                    })
                    .await?;
                serde_json::to_value(v).map_err(Into::into)
            }
            "react" => {
                self.service
                    .react(ReactParams {
                        account: need(args.account, "account")?,
                        platform_id: need(args.platform_id, "platform_id")?,
                        emoji: need(args.emoji, "emoji")?,
                    })
                    .await?;
                Ok(serde_json::json!({"ok": true}))
            }
            "conversations" => {
                serde_json::to_value(self.service.conversations(ConversationsParams {
                    account: args.account,
                })?)
                .map_err(Into::into)
            }
            other => Err(ChannelError::Invalid(format!(
                "op must be list|capabilities|blob_upload_url|send|get|history|search|recall|react|conversations (got {other})"
            ))),
        }
    }
}

#[tool_handler]
impl ServerHandler for ChannelMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "GLaDYS channel. Tool channel: op=list|capabilities|blob_upload_url|send|get|history|search|recall|react|conversations. send uses parts, never CQ codes in text. send reply = platform message id. Media: op=blob_upload_url, PUT file, send blob_id. Native: channel_call.",
            )
    }
}
