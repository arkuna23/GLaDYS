use async_trait::async_trait;
use reqwest::Client;

use crate::error::{GatewayError, Result};
use crate::io::MemoryIo;
use crate::types::{ConversationKind, Pack};

pub struct MemoryHttp {
    base: String,
    token: String,
    http: Client,
}

impl MemoryHttp {
    pub fn new(base: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').into(),
            token: token.into(),
            http: Client::new(),
        }
    }
}

#[async_trait]
impl MemoryIo for MemoryHttp {
    async fn pack(
        &self,
        channel: &str,
        kind: ConversationKind,
        peer: &str,
        person: Option<&str>,
    ) -> Result<Pack> {
        let mut url = format!(
            "{}/v1/pack?channel={}&kind={}&peer={}",
            self.base,
            urlencoding_lite(channel),
            kind.as_str(),
            urlencoding_lite(peer)
        );
        match kind {
            ConversationKind::Group => {}
            ConversationKind::Dm => {
                let who = person.unwrap_or(peer);
                url.push_str("&person=");
                url.push_str(&urlencoding_lite(who));
            }
        }
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(GatewayError::Memory(format!("pack {}", resp.status())));
        }
        Ok(resp.json().await?)
    }
}

fn urlencoding_lite(s: &str) -> String {
    s.replace(' ', "%20")
}
