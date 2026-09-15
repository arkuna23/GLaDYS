use reqwest::Client;
use serde_json::Value;

use crate::error::{Result, SchedulerError};

#[derive(Clone)]
pub struct GatewayClient {
    http: Client,
    base: String,
    token: String,
}

impl GatewayClient {
    pub fn new(base: String, token: String) -> Self {
        Self {
            http: Client::new(),
            base: base.trim_end_matches('/').to_string(),
            token,
        }
    }

    pub async fn create(&self, body: Value) -> Result<Value> {
        self.send(
            self.http
                .post(format!("{}/v1/scheduler/jobs", self.base))
                .json(&body),
        )
        .await
    }

    pub async fn list(&self) -> Result<Value> {
        self.send(self.http.get(format!("{}/v1/scheduler/jobs", self.base)))
            .await
    }

    pub async fn cancel(&self, id: &str) -> Result<Value> {
        self.send(
            self.http
                .delete(format!("{}/v1/scheduler/jobs/{id}", self.base)),
        )
        .await
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value> {
        let resp = req
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await?;
        let status = resp.status();
        let v = resp.json::<Value>().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(SchedulerError::Gateway(format!("{status}: {v}")));
        }
        Ok(v)
    }
}
