use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Global,
    Conversation,
    Person,
}

impl Layer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Conversation => "conversation",
            Self::Person => "person",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "global" => Some(Self::Global),
            "conversation" => Some(Self::Conversation),
            "person" => Some(Self::Person),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Conversation {
    pub kind: ConversationKind,
    pub peer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub layer: Layer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<Conversation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
    pub text: String,
    pub ts: i64,
}

impl Memory {
    pub fn new_id() -> String {
        Ulid::generate().to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pack {
    pub global: Vec<Memory>,
    pub conversation: Vec<Memory>,
    pub person: Vec<Memory>,
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRef {
    pub channel: String,
    pub kind: ConversationKind,
    pub peer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonRef {
    pub channel: String,
    pub person: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scopes {
    pub channels: Vec<String>,
    pub conversations: Vec<ConversationRef>,
    pub persons: Vec<PersonRef>,
}
