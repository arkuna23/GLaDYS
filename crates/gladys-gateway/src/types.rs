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
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
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
}

impl Envelope {
    pub fn key(&self) -> ConvKey {
        ConvKey::new(&self.channel, &self.conversation)
    }

    pub fn flatten_text(&self) -> String {
        flatten_parts(&self.parts)
    }

    pub fn mentions_user(&self, user_id: &str) -> bool {
        self.parts.iter().any(|p| matches!(p, Part::Mention { target: MentionTarget::User, id: Some(id) } if id == user_id))
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
            _ => {}
        }
    }
    out
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
}

#[derive(Debug, Clone)]
pub struct AgentInput {
    pub idle: bool,
    pub account: String,
    pub key: ConvKey,
    pub messages: Vec<Envelope>,
    pub pack: Pack,
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
