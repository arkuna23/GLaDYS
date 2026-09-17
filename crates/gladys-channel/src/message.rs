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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
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

    pub fn flatten_cq(&self) -> String {
        flatten_cq(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchMsg {
    pub id: String,
    pub ts: i64,
    pub dir: String,
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchGroup {
    pub channel: String,
    pub account: String,
    pub kind: String,
    pub peer: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub names: std::collections::BTreeMap<String, String>,
    pub messages: Vec<SearchMsg>,
}

impl SearchGroup {
    pub fn from_envelopes(envs: &[Envelope]) -> Self {
        let first = &envs[0];
        let mut names = std::collections::BTreeMap::new();
        let messages = envs
            .iter()
            .map(|e| {
                if let Some(n) = e.sender.name.as_deref().filter(|s| !s.is_empty()) {
                    names.insert(e.sender.id.clone(), n.to_string());
                }
                SearchMsg {
                    id: e.id.clone(),
                    ts: e.ts,
                    dir: match e.direction {
                        Direction::In => "in".into(),
                        Direction::Out => "out".into(),
                    },
                    from: e.sender.id.clone(),
                    pid: e.platform_id.clone(),
                    text: e.flatten_cq(),
                }
            })
            .collect();
        Self {
            channel: first.channel.clone(),
            account: first.account.clone(),
            kind: first.conversation.kind_str().into(),
            peer: first.conversation.peer.clone(),
            names,
            messages,
        }
    }

    pub fn empty(
        channel: impl Into<String>,
        account: impl Into<String>,
        kind: impl Into<String>,
        peer: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            account: account.into(),
            kind: kind.into(),
            peer: peer.into(),
            names: std::collections::BTreeMap::new(),
            messages: Vec::new(),
        }
    }
}

pub fn flatten_cq(env: &Envelope) -> String {
    let mut out = String::new();
    let reply_id = env
        .reply_to
        .as_ref()
        .and_then(|r| r.platform_id.as_deref())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            env.parts.iter().find_map(|p| match p {
                Part::Reply { platform_id } if !platform_id.is_empty() => Some(platform_id.as_str()),
                _ => None,
            })
        });
    if let Some(id) = reply_id {
        out.push_str(&cq("reply", &[("id", id)]));
    }
    for part in &env.parts {
        match part {
            Part::Reply { .. } => {}
            Part::Text { text } => out.push_str(&cq_escape_text(text)),
            Part::Mention {
                target: MentionTarget::All,
                ..
            } => out.push_str(&cq("at", &[("qq", "all")])),
            Part::Mention {
                target: MentionTarget::User,
                id,
            } => out.push_str(&cq("at", &[("qq", id.as_deref().unwrap_or("user"))])),
            Part::Image {
                url,
                blob_id,
                filename,
                ..
            } => out.push_str(&media_cq("image", url, blob_id, filename)),
            Part::Audio {
                url,
                blob_id,
                filename,
                ..
            } => out.push_str(&media_cq("record", url, blob_id, filename)),
            Part::Video {
                url,
                blob_id,
                filename,
                ..
            } => out.push_str(&media_cq("video", url, blob_id, filename)),
            Part::File {
                url,
                blob_id,
                filename,
                ..
            } => out.push_str(&media_cq("file", url, blob_id, filename)),
            Part::Unknown { native_type, data } => out.push_str(&unknown_cq(native_type, data)),
        }
    }
    out
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


fn cq_escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('[', "&#91;").replace(']', "&#93;")
}

fn cq_escape_val(s: &str) -> String {
    cq_escape_text(s).replace(',', "&#44;")
}

fn cq(ty: &str, params: &[(&str, &str)]) -> String {
    let mut out = String::from("[CQ:");
    out.push_str(ty);
    for (k, v) in params {
        if v.is_empty() {
            continue;
        }
        out.push(',');
        out.push_str(k);
        out.push('=');
        out.push_str(&cq_escape_val(v));
    }
    out.push(']');
    out
}

fn media_cq(kind: &str, url: &Option<String>, blob_id: &Option<String>, filename: &Option<String>) -> String {
    if let Some(u) = url.as_deref().filter(|s| !s.is_empty()) {
        return cq(kind, &[("url", u)]);
    }
    if let Some(f) = filename.as_deref().filter(|s| !s.is_empty()) {
        return cq(kind, &[("file", f)]);
    }
    if let Some(b) = blob_id.as_deref().filter(|s| !s.is_empty()) {
        return cq(kind, &[("file", b)]);
    }
    cq(kind, &[])
}

fn unknown_cq(native_type: &str, data: &Value) -> String {
    let mut params = Vec::new();
    if let Some(obj) = data.as_object() {
        for (k, v) in obj {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            params.push((k.as_str(), s));
        }
    }
    let refs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    cq(native_type, &refs)
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


#[cfg(test)]
mod tests {
    use super::*;

    fn env_parts(parts: Vec<Part>, reply: Option<&str>) -> Envelope {
        Envelope {
            id: "m1".into(),
            channel: "onebot".into(),
            account: "qq".into(),
            conversation: Conversation::group("1"),
            direction: Direction::In,
            sender: Actor {
                id: "u".into(),
                name: None,
            },
            ts: 1,
            platform_id: Some("9".into()),
            reply_to: reply.map(|id| ReplyTo {
                id: None,
                platform_id: Some(id.into()),
                sender: None,
            }),
            parts,
            native: None,
            recalled: false,
        }
    }

    #[test]
    fn flatten_cq_reply_first_keeps_image_url() {
        let e = env_parts(
            vec![
                Part::Text { text: "看".into() },
                Part::Image {
                    blob_id: None,
                    url: Some("http://x".into()),
                    mime: None,
                    filename: None,
                },
            ],
            Some("9"),
        );
        assert_eq!(
            e.flatten_cq(),
            "[CQ:reply,id=9]看[CQ:image,url=http://x]"
        );
    }
}
