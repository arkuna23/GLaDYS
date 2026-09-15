use std::sync::Arc;

use axum::extract::{Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
use rmcp::transport::StreamableHttpServerConfig;
use serde::Deserialize;

use crate::mcp::MemoryMcp;
use crate::service::{PackParams, Service};
use crate::types::{Conversation, ConversationKind};

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<Service>,
    pub token: String,
}

pub fn router(state: AppState) -> Router {
    let mcp_state = state.clone();
    let mcp: StreamableHttpService<MemoryMcp, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(MemoryMcp::new(mcp_state.service.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let protected_mcp = Router::new().nest_service("/mcp", mcp).layer(
        middleware::from_fn_with_state(state.clone(), auth_middleware),
    );
    Router::new()
        .route("/health", get(health))
        .route("/v1/pack", get(pack))
        .merge(protected_mcp)
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if token == Some(state.token.as_str()) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[derive(Deserialize)]
struct PackQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    peer: Option<String>,
    #[serde(default)]
    person: Option<String>,
}

async fn pack(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<PackQuery>,
) -> Result<Json<crate::types::Pack>, StatusCode> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if token != Some(state.token.as_str()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let conversation = match (q.kind.as_deref().and_then(ConversationKind::parse), q.peer) {
        (Some(kind), Some(peer)) => Some(Conversation { kind, peer }),
        _ => None,
    };
    state
        .service
        .pack(PackParams {
            channel: q.channel,
            conversation,
            person: q.person,
        })
        .map(Json)
        .map_err(|_| StatusCode::BAD_REQUEST)
}

pub async fn serve(bind: &str, state: AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("memory server on {bind}");
    tokio::select! {
        result = axum::serve(listener, router(state)) => result?,
        _ = shutdown_signal() => {
            tracing::info!("shutting down");
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("sigterm handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}
