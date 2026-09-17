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

fn need<T>(v: Option<T>, name: &str) -> Result<T, MemoryError> {
    v.ok_or_else(|| MemoryError::Invalid(format!("{name} required")))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MemoryArgs {
    /// write | get | search | forget | pack
    op: String,
    #[serde(default)]
    layer: Option<Layer>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    conversation: Option<Conversation>,
    #[serde(default)]
    person: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[tool_router]
impl MemoryMcp {
    #[tool(
        description = "Memory ops. op=write|get|search|forget|pack. Layers: global, conversation (channel+kind+peer), person (channel+person). write needs layer+text; get/forget need id; search needs query."
    )]
    fn memory(
        &self,
        Parameters(args): Parameters<MemoryArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.dispatch(args))
    }
}

impl MemoryMcp {
    fn dispatch(&self, args: MemoryArgs) -> Result<Value, MemoryError> {
        match args.op.as_str() {
            "write" => serde_json::to_value(self.service.write(WriteParams {
                layer: need(args.layer, "layer")?,
                text: need(args.text, "text")?,
                channel: args.channel,
                conversation: args.conversation,
                person: args.person,
            })?)
            .map_err(Into::into),
            "get" => serde_json::to_value(self.service.get(GetParams {
                id: need(args.id, "id")?,
            })?)
            .map_err(Into::into),
            "search" => serde_json::to_value(self.service.search(SearchParams {
                query: need(args.query, "query")?,
                layer: args.layer,
                channel: args.channel,
                conversation: args.conversation,
                person: args.person,
                limit: args.limit.unwrap_or(50),
            })?)
            .map_err(Into::into),
            "forget" => {
                self.service
                    .forget(ForgetParams {
                        id: need(args.id, "id")?,
                    })?;
                Ok(serde_json::json!({"ok": true}))
            }
            "pack" => serde_json::to_value(self.service.pack(PackParams {
                channel: args.channel,
                conversation: args.conversation,
                person: args.person,
            })?)
            .map_err(Into::into),
            other => Err(MemoryError::Invalid(format!(
                "op must be write|get|search|forget|pack (got {other})"
            ))),
        }
    }
}

#[tool_handler]
impl ServerHandler for MemoryMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "GLaDYS memory. Tool memory: op=write|get|search|forget|pack. Layers: global, conversation (channel+kind+peer), person (channel+person).",
            )
    }
}
