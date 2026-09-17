use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::GatewayClient;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub kind: String,
    pub name: String,
    pub account: String,
    pub channel: String,
    pub conversation: Conversation,
    #[serde(default)]
    pub sender: Actor,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub lang: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub kind: String,
    pub peer: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Actor {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

pub fn fill_channel_args(args: &mut Value, req: &RunRequest) {
    let Some(obj) = args.as_object_mut() else {
        return;
    };
    obj.entry("account")
        .or_insert_with(|| json!(req.account));
    obj.entry("conversation").or_insert_with(|| {
        json!({"kind": req.conversation.kind, "peer": req.conversation.peer})
    });
}

pub fn fill_memory_args(args: &mut Value, req: &RunRequest) {
    let Some(obj) = args.as_object_mut() else {
        return;
    };
    obj.entry("channel")
        .or_insert_with(|| json!(req.channel));
    obj.entry("conversation").or_insert_with(|| {
        json!({"kind": req.conversation.kind, "peer": req.conversation.peer})
    });
    if req.conversation.kind == "dm" {
        obj.entry("person")
            .or_insert_with(|| json!(req.sender.id));
    }
}

pub fn fill_job_args(args: &mut Value, req: &RunRequest) {
    let Some(obj) = args.as_object_mut() else {
        return;
    };
    obj.entry("account")
        .or_insert_with(|| json!(req.account));
    obj.entry("channel")
        .or_insert_with(|| json!(req.channel));
    obj.entry("conversation").or_insert_with(|| {
        json!({"kind": req.conversation.kind, "peer": req.conversation.peer})
    });
}

pub async fn exec_script(
    script: &str,
    req: &RunRequest,
    channel: crate::channel_mcp::SharedTools,
    memory: crate::channel_mcp::SharedTools,
    gateway: GatewayClient,
    timeout: std::time::Duration,
) -> Result<()> {
    crate::lua::run(script, req, channel, memory, gateway, timeout).await
}
