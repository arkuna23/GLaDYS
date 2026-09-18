use std::sync::Arc;

use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
use rmcp::transport::StreamableHttpServerConfig;
use serde::Deserialize;

use crate::error::MemoryError;
use crate::mcp::MemoryMcp;
use crate::service::{ListParams, PackParams, PatchParams, Service, WriteParams};
use crate::types::{Conversation, ConversationKind, Layer, Memory};

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
    let protected = Router::new()
        .nest_service("/mcp", mcp)
        .route("/v1/memories", get(list_memories).post(write_memory))
        .route(
            "/v1/memories/{id}",
            get(get_memory).patch(patch_memory).delete(delete_memory),
        )
        .route("/v1/scopes", get(scopes))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    Router::new()
        .route("/health", get(health))
        .route("/v1/pack", get(pack))
        .merge(protected)
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

fn map_err(e: MemoryError) -> StatusCode {
    match e {
        MemoryError::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::BAD_REQUEST,
    }
}

#[derive(Deserialize)]
struct MemoriesQuery {
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    peer: Option<String>,
    #[serde(default)]
    person: Option<String>,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    before_ts: Option<i64>,
}

async fn list_memories(
    State(state): State<AppState>,
    Query(q): Query<MemoriesQuery>,
) -> Result<Json<Vec<Memory>>, StatusCode> {
    let layer = match q.layer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => Some(Layer::parse(raw).ok_or(StatusCode::BAD_REQUEST)?),
        None => None,
    };
    let conversation = match (q.kind.as_deref().and_then(ConversationKind::parse), q.peer) {
        (Some(kind), Some(peer)) if !peer.is_empty() => Some(Conversation { kind, peer }),
        _ => None,
    };
    state
        .service
        .list(ListParams {
            layer,
            channel: q.channel.filter(|s| !s.is_empty()),
            conversation,
            person: q.person.filter(|s| !s.is_empty()),
            query: q.q,
            limit: q.limit.unwrap_or(50),
            before_ts: q.before_ts,
        })
        .map(Json)
        .map_err(map_err)
}

async fn write_memory(
    State(state): State<AppState>,
    Json(body): Json<WriteParams>,
) -> Result<Json<Memory>, StatusCode> {
    state.service.write(body).map(Json).map_err(map_err)
}

async fn get_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Memory>, StatusCode> {
    state
        .service
        .get(crate::service::GetParams { id })
        .map(Json)
        .map_err(map_err)
}

async fn patch_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchParams>,
) -> Result<Json<Memory>, StatusCode> {
    state
        .service
        .update_text(&id, body)
        .map(Json)
        .map_err(map_err)
}

async fn delete_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .service
        .forget(crate::service::ForgetParams { id })
        .map(|_| Json(serde_json::json!({"ok": true})))
        .map_err(map_err)
}

async fn scopes(
    State(state): State<AppState>,
) -> Result<Json<crate::types::Scopes>, StatusCode> {
    state.service.scopes().map(Json).map_err(map_err)

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
