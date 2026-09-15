use serde::{Deserialize, Serialize};
use serde_json::Value;
use ulid::Ulid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    Dm,
    Group,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Conversation {
    pub kind: ConversationKind,
    pub peer: String,
}

impl Conversation {
    pub fn dm(peer: impl Into<String>) -> Self {
        Self {
            kind: ConversationKind::Dm,
            peer: peer.into(),
        }
    }

    pub fn group(peer: impl Into<String>) -> Self {
        Self {
            kind: ConversationKind::Group,
            peer: peer.into(),
        }
    }

    pub fn key(&self, channel: &str, account: &str) -> String {
        format!("{channel}:{account}:{}:{}", self.kind_str(), self.peer)
    }

    pub fn kind_str(&self) -> &'static str {
        match self.kind {
            ConversationKind::Dm => "dm",
            ConversationKind::Group => "group",
        }
    }

    pub fn from_parts(kind: &str, peer: impl Into<String>) -> Option<Self> {
        let kind = match kind {
            "dm" | "private" => ConversationKind::Dm,
            "group" => ConversationKind::Group,
            _ => return None,
        };
        Some(Self {
            kind,
            peer: peer.into(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    In,
    Out,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyTo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MentionTarget {
    All,
    User,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text {
        text: String,
    },
    Mention {
        target: MentionTarget,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    Image {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blob_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
    Audio {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blob_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
    Video {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blob_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
    File {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blob_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
    Reply {
        platform_id: String,
    },
    Unknown {
        native_type: String,
        #[serde(default)]
        data: Value,
    },
}

impl Part {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativePayload {
    #[serde(rename = "type")]
    pub kind: String,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub id: String,
    pub channel: String,
    pub account: String,
    pub conversation: Conversation,
    pub direction: Direction,
    pub sender: Actor,
    pub ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyTo>,
    pub parts: Vec<Part>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativePayload>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub recalled: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

impl Envelope {
    pub fn new_id() -> String {
        Ulid::generate().to_string()
    }

    pub fn flattened_text(&self) -> String {
        flatten_parts(&self.parts)
    }
}

pub fn flatten_parts(parts: &[Part]) -> String {
    let mut out = String::new();
    for part in parts {
        match part {
            Part::Text { text } => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(text);
            }
            Part::Mention {
                target: MentionTarget::All,
                ..
            } => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str("@all");
            }
            Part::Mention {
                target: MentionTarget::User,
                id,
            } => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push('@');
                out.push_str(id.as_deref().unwrap_or("user"));
            }
            Part::Reply { platform_id } => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str("[reply:");
                out.push_str(platform_id);
                out.push(']');
            }
            _ => {}
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlatformEvent {
    pub id: String,
    pub channel: String,
    pub account: String,
    pub ts: i64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<Conversation>,
    pub native: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationInfo {
    pub channel: String,
    pub account: String,
    pub conversation: Conversation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub last_ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroppedPart {
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendAck {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<DroppedPart>,
}

#[derive(Debug, Clone)]
pub enum Ingress {
    Message(Envelope),
    Notice(PlatformEvent),
    Request(PlatformEvent),
    Connection {
        account: String,
        up: bool,
        detail: String,
        self_id: Option<String>,
    },
}
