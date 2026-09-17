use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app::App;
use crate::error::{DaemonError, Result};

#[derive(Clone)]
pub struct DaemonMcp {
    app: App,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl DaemonMcp {
    pub fn new(app: App) -> Self {
        Self {
            app,
            tool_router: Self::tool_router(),
        }
    }

    async fn wrap(result: Result<Value>) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
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

    async fn after(&self) {
        self.app.push_registry().await;
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct Conversation {
    kind: String,
    peer: String,
}

fn conv(c: Conversation) -> Value {
    json!({ "kind": c.kind, "peer": c.peer })
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DocsArgs {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScriptArgs {
    /// list | get | delete
    op: String,
    /// command | handler
    #[serde(rename = "type")]
    type_name: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct JobArgs {
    /// cron | delay | list | cancel
    op: String,
    #[serde(default)]
    account: String,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    conversation: Option<Conversation>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    cron: Option<String>,
    #[serde(default)]
    after_secs: Option<u64>,
    #[serde(default)]
    id: Option<String>,
}

#[tool_router]
impl DaemonMcp {
    #[tool(
        description = "Lua API and how to upload scripts. Optional name filters to one stored command/handler."
    )]
    async fn daemon_docs(
        &self,
        Parameters(args): Parameters<DocsArgs>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(
            self.app
                .store
                .docs(args.name.as_deref())
                .map(|text| json!({ "text": text })),
        )
        .await
    }

    #[tool(
        description = "List/get/delete Lua commands or handlers. op=list|get|delete; type=command|handler; name for get/delete. Save via HTTP PUT /v1/scripts/{name}."
    )]
    async fn daemon_script(
        &self,
        Parameters(args): Parameters<ScriptArgs>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let r = self.script_op(args);
        if matches!(&r, Ok(v) if v.get("ok") == Some(&json!(true))) {
            self.after().await;
        }
        Self::wrap(r).await
    }

    #[tool(
        description = "Gateway jobs. op=cron|delay|list|cancel. cron needs cron+account+channel+conversation+text; delay needs after_secs+same; cancel needs id."
    )]
    async fn daemon_job(
        &self,
        Parameters(args): Parameters<JobArgs>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        Self::wrap(self.job_op(args).await).await
    }
}

impl DaemonMcp {
    fn script_op(&self, args: ScriptArgs) -> Result<Value> {
        match (args.op.as_str(), args.type_name.as_str()) {
            ("list", "command") => self
                .app
                .store
                .list_commands()
                .map(|v| json!({ "data": v })),
            ("list", "handler") => self
                .app
                .store
                .list_handlers()
                .map(|v| json!({ "data": v })),
            ("get", "command") => {
                let name = args.name.as_deref().ok_or_else(|| DaemonError::Invalid("name required".into()))?;
                self.app.store.get_command(name).map(|c| json!(c))
            }
            ("get", "handler") => {
                let name = args.name.as_deref().ok_or_else(|| DaemonError::Invalid("name required".into()))?;
                self.app.store.get_handler(name).map(|h| json!(h))
            }
            ("delete", "command") => {
                let name = args.name.as_deref().ok_or_else(|| DaemonError::Invalid("name required".into()))?;
                self.app.store.delete_command(name).map(|ok| json!({ "ok": ok }))
            }
            ("delete", "handler") => {
                let name = args.name.as_deref().ok_or_else(|| DaemonError::Invalid("name required".into()))?;
                self.app.store.delete_handler(name).map(|ok| json!({ "ok": ok }))
            }
            (op, ty) => Err(DaemonError::Invalid(format!(
                "op must be list|get|delete, type command|handler (got {op}/{ty})"
            ))),
        }
    }

    async fn job_op(&self, args: JobArgs) -> Result<Value> {
        match args.op.as_str() {
            "list" => self.app.gateway.list().await,
            "cancel" => {
                let id = args
                    .id
                    .as_deref()
                    .ok_or_else(|| DaemonError::Invalid("id required".into()))?;
                self.app.gateway.cancel(id).await
            }
            "cron" => {
                let conversation = args
                    .conversation
                    .ok_or_else(|| DaemonError::Invalid("conversation required".into()))?;
                let cron = args
                    .cron
                    .ok_or_else(|| DaemonError::Invalid("cron required".into()))?;
                self.app
                    .gateway
                    .create(json!({
                        "kind": "cron",
                        "account": args.account,
                        "channel": args.channel,
                        "conversation": conv(conversation),
                        "text": args.text,
                        "cron": cron,
                    }))
                    .await
            }
            "delay" => {
                let conversation = args
                    .conversation
                    .ok_or_else(|| DaemonError::Invalid("conversation required".into()))?;
                let after_secs = args
                    .after_secs
                    .ok_or_else(|| DaemonError::Invalid("after_secs required".into()))?;
                self.app
                    .gateway
                    .create(json!({
                        "kind": "delay",
                        "account": args.account,
                        "channel": args.channel,
                        "conversation": conv(conversation),
                        "text": args.text,
                        "after_secs": after_secs,
                    }))
                    .await
            }
            other => Err(DaemonError::Invalid(format!(
                "op must be cron|delay|list|cancel (got {other})"
            ))),
        }
    }
}

#[tool_handler]
impl ServerHandler for DaemonMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "GLaDYS daemon. Save Lua with PUT /v1/scripts/{name} (header X-Gladys-Type=command|handler). Check with POST /v1/scripts/check. Read daemon_docs. Jobs: daemon_job.",
            )
    }
}
