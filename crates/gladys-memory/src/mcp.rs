use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::Deserialize;
use serde_json::Value;

use crate::error::MemoryError;
use crate::service::{
    ForgetParams, GetParams, PackParams, SearchParams, Service, WriteParams,
};
use crate::types::{Conversation, Layer};

#[derive(Clone)]
pub struct MemoryMcp {
    service: Arc<Service>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl MemoryMcp {
    pub fn new(service: Arc<Service>) -> Self {
        Self {
            service,
            tool_router: Self::tool_router(),
        }
    }

    fn wrap<T: serde::Serialize>(
        result: Result<T, MemoryError>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
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
struct WriteArgs {
    layer: Layer,
    text: String,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    conversation: Option<Conversation>,
    #[serde(default)]
    person: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct IdArgs {
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    layer: Option<Layer>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    conversation: Option<Conversation>,
    #[serde(default)]
    person: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PackArgs {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    conversation: Option<Conversation>,
    #[serde(default)]
    person: Option<String>,
}

#[tool_router]
impl MemoryMcp {
    #[tool(description = "Write a memory note into global, conversation, or person layer")]
    fn memory_write(
        &self,
        Parameters(args): Parameters<WriteArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.service.write(WriteParams {
            layer: args.layer,
            text: args.text,
            channel: args.channel,
            conversation: args.conversation,
            person: args.person,
        }))
    }

    #[tool(description = "Get one memory by id")]
    fn memory_get(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.service.get(GetParams { id: args.id }))
    }

    #[tool(description = "Full-text search memories")]
    fn memory_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.service.search(SearchParams {
            query: args.query,
            layer: args.layer,
            channel: args.channel,
            conversation: args.conversation,
            person: args.person,
            limit: args.limit.unwrap_or(50),
        }))
    }

    #[tool(description = "Delete a memory by id")]
    fn memory_forget(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(
            self.service
                .forget(ForgetParams { id: args.id })
                .map(|()| serde_json::json!({"ok": true})),
        )
    }

    #[tool(description = "Pack global + conversation + person memories for the current context")]
    fn memory_pack(
        &self,
        Parameters(args): Parameters<PackArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.service.pack(PackParams {
            channel: args.channel,
            conversation: args.conversation,
            person: args.person,
        }))
    }
}

#[tool_handler]
impl ServerHandler for MemoryMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "GLaDYS memory. Layers: global, conversation (channel+kind+peer), person (channel+person).",
            )
    }
}
