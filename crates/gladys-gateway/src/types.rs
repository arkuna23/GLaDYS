use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    Dm,
    Group,
}

impl ConversationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dm => "dm",
            Self::Group => "group",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "dm" | "private" => Some(Self::Dm),
            "group" => Some(Self::Group),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Conversation {
    pub kind: ConversationKind,
    pub peer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConvKey {
    pub channel: String,
    pub kind: ConversationKind,
    pub peer: String,
}

impl ConvKey {
    pub fn new(channel: impl Into<String>, conv: &Conversation) -> Self {
        Self {
            channel: channel.into(),
            kind: conv.kind,
            peer: conv.peer.clone(),
        }
    }

    pub fn as_list_key(&self) -> String {
        format!("{}:{}:{}", self.channel, self.kind.as_str(), self.peer)
    }

    pub fn owner_key(&self, person: &str) -> String {
        format!("{}:{person}", self.channel)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MentionTarget {
    All,
    User,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text {
        text: String,
    },
    Mention {
        target: MentionTarget,
        #[serde(default)]
        id: Option<String>,
    },
    Image {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        blob_id: Option<String>,
        #[serde(default)]
        mime: Option<String>,
        #[serde(default)]
        filename: Option<String>,
    },
    Audio {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        blob_id: Option<String>,
        #[serde(default)]
        mime: Option<String>,
        #[serde(default)]
        filename: Option<String>,
    },
    Video {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        blob_id: Option<String>,
        #[serde(default)]
        mime: Option<String>,
        #[serde(default)]
        filename: Option<String>,
    },
    File {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        blob_id: Option<String>,
        #[serde(default)]
        mime: Option<String>,
        #[serde(default)]
        filename: Option<String>,
    },
    Reply {
        platform_id: String,
    },
    Unknown {
        native_type: String,
        #[serde(default)]
        data: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ReplyTo {
    #[serde(default)]
    pub platform_id: Option<String>,
    #[serde(default)]
    pub sender: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(default)]
    pub id: String,
    pub channel: String,
    pub account: String,
    pub conversation: Conversation,
    #[serde(default)]
    pub direction: String,
    pub sender: Actor,
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub reply_to: Option<ReplyTo>,
}

impl Envelope {
    pub fn key(&self) -> ConvKey {
        ConvKey::new(&self.channel, &self.conversation)
    }

    pub fn flatten_text(&self) -> String {
        flatten_parts(&self.parts)
    }

    pub fn prompt_line(&self) -> String {
        format!(
            "{}[{}]: {}",
            self.sender.name.as_deref().unwrap_or(""),
            self.sender.id,
            self.flatten_text()
        )
    }

    pub fn mentions_user(&self, user_id: &str) -> bool {
        self.parts.iter().any(|p| matches!(p, Part::Mention { target: MentionTarget::User, id: Some(id) } if id == user_id))
    }

    pub fn replies_to(&self, user_id: &str) -> bool {
        self.reply_to.as_ref().and_then(|r| r.sender.as_deref()) == Some(user_id)
    }
}

pub fn flatten_parts(parts: &[Part]) -> String {
    let mut out = String::new();
    for part in parts {
        match part {
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
            Part::Reply { platform_id } => out.push_str(&cq("reply", &[("id", platform_id)])),
            Part::Unknown { native_type, data } => out.push_str(&unknown_cq(native_type, data)),
            Part::Other => out.push_str("[CQ:other]"),
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

fn media_cq(
    kind: &str,
    url: &Option<String>,
    blob_id: &Option<String>,
    filename: &Option<String>,
 ) -> String {
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

fn unknown_cq(native_type: &str, data: &serde_json::Value) -> String {
    let mut params = Vec::new();
    if let Some(obj) = data.as_object() {
        for (k, v) in obj {
            let s = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            params.push((k.as_str(), s));
        }
    }
    let refs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    cq(native_type, &refs)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub ts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Pack {
    #[serde(default)]
    pub global: Vec<MemoryItem>,
    #[serde(default)]
    pub conversation: Vec<MemoryItem>,
    #[serde(default)]
    pub person: Vec<MemoryItem>,
}

impl Pack {
    pub fn render(&self) -> String {
        let mut s = String::new();
        for (label, items) in [
            ("global", &self.global),
            ("conversation", &self.conversation),
            ("person", &self.person),
        ] {
            if items.is_empty() {
                continue;
            }
            s.push_str(label);
            s.push('\n');
            for it in items {
                s.push_str("- ");
                s.push_str(&it.text);
                s.push('\n');
            }
        }
        s
    }

    pub fn global_delta(prev: &Pack, next: &Pack) -> PackDelta {
        use std::collections::HashMap;
        let old: HashMap<&str, &MemoryItem> =
            prev.global.iter().map(|m| (m.id.as_str(), m)).collect();
        let new: HashMap<&str, &MemoryItem> =
            next.global.iter().map(|m| (m.id.as_str(), m)).collect();
        let mut delta = PackDelta::default();
        for (id, item) in &new {
            match old.get(id) {
                None => delta.added.push((*item).clone()),
                Some(prev_item) if prev_item.text != item.text => {
                    delta
                        .changed
                        .push(((*prev_item).clone(), (*item).clone()));
                }
                Some(_) => {}
            }
        }
        for (id, item) in &old {
            if !new.contains_key(id) {
                delta.removed.push((*item).clone());
            }
        }
        delta
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackDelta {
    pub added: Vec<MemoryItem>,
    pub changed: Vec<(MemoryItem, MemoryItem)>,
    pub removed: Vec<MemoryItem>,
}

impl PackDelta {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for it in &self.added {
            s.push_str("+ global: ");
            s.push_str(&it.text);
            s.push('\n');
        }
        for (old, new) in &self.changed {
            s.push_str("~ global: ");
            s.push_str(&old.text);
            s.push_str(" => ");
            s.push_str(&new.text);
            s.push('\n');
        }
        for it in &self.removed {
            s.push_str("- global: ");
            s.push_str(&it.text);
            s.push('\n');
        }
        s
    }
}

#[derive(Debug, Clone)]
pub struct AgentInput {
    pub idle: bool,
    pub dream: bool,
    pub account: String,
    pub self_id: Option<String>,
    pub key: ConvKey,
    pub messages: Vec<Envelope>,
    pub pack: Pack,
    pub pack_delta: PackDelta,
}
#[derive(Debug, Clone, Default)]
pub struct AgentOutput {
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMode {
    Whitelist,
    Blacklist,
}

impl ListMode {
    pub fn parse(raw: Option<&str>, default: Self) -> Self {
        match raw.map(str::to_ascii_lowercase).as_deref() {
            Some("whitelist") => Self::Whitelist,
            Some("blacklist") => Self::Blacklist,
            _ => default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_uses_cq() {
        let s = flatten_parts(&[
            Part::Reply {
                platform_id: "9".into(),
            },
            Part::Mention {
                target: MentionTarget::User,
                id: Some("1".into()),
            },
            Part::Text {
                text: "看".into(),
            },
            Part::Image {
                url: Some("http://x".into()),
                blob_id: None,
                mime: None,
                filename: None,
            },
            Part::Unknown {
                native_type: "face".into(),
                data: serde_json::json!({"id": "32"}),
            },
        ]);
        assert_eq!(
            s,
            "[CQ:reply,id=9][CQ:at,qq=1]看[CQ:image,url=http://x][CQ:face,id=32]"
        );
    }
}
