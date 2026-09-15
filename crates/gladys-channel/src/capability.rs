use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Public,
    Napcat,
}

impl Profile {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::to_ascii_lowercase).as_deref() {
            Some("public") => Self::Public,
            _ => Self::Napcat,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Napcat => "napcat",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommonCaps {
    pub send: bool,
    pub get: bool,
    pub history: bool,
    pub search: bool,
    pub recall: bool,
    pub react: bool,
    pub conversations: bool,
}

impl CommonCaps {
    pub fn full() -> Self {
        Self {
            send: true,
            get: true,
            history: true,
            search: true,
            recall: true,
            react: true,
            conversations: true,
        }
    }

    pub fn onebot_public() -> Self {
        Self {
            react: false,
            ..Self::full()
        }
    }

    pub fn supports(&self, op: &str) -> bool {
        match op {
            "send" => self.send,
            "get" => self.get,
            "history" => self.history,
            "search" => self.search,
            "recall" => self.recall,
            "react" => self.react,
            "conversations" => self.conversations,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeOp {
    pub name: String,
    pub description: String,
    pub params_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub channel: String,
    pub account: String,
    pub profile: String,
    pub common: CommonCaps,
    pub native_ops: Vec<NativeOp>,
    pub native_message_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountStatus {
    pub id: String,
    pub channel: String,
    pub profile: String,
    pub up: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_id: Option<String>,
}
