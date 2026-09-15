use std::sync::Arc;

use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
use rmcp::transport::StreamableHttpServerConfig;
use serde::Deserialize;

use crate::error::ChannelError;
use crate::gateway_ws;
use crate::mcp::ChannelMcp;
use crate::message::{Envelope, Ingress};
use crate::service::Service;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<Service>,
    pub token: String,
    pub debug: bool,
    pub loopback_blobs: bool,
}

pub fn router(state: AppState) -> Router {
    let mcp_state = state.clone();
    let mcp: StreamableHttpService<ChannelMcp, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(ChannelMcp::new(mcp_state.service.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );

    let protected_mcp = Router::new()
        .nest_service("/mcp", mcp)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let mut app = Router::new()
        .route("/health", get(health))
        .route("/v1/gateway", get(gateway_ws::upgrade))
        .route("/v1/blobs/{id}", get(get_blob));
    if state.debug {
        app = app.route("/v1/debug/inbound", post(debug_inbound));
    }
    app.merge(protected_mcp).with_state(state)
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
struct BlobQuery {
    #[serde(default)]
    access_token: Option<String>,
}

fn blob_allowed(state: &AppState, headers: &HeaderMap, query_token: Option<&str>) -> bool {
    if state.loopback_blobs {
        return true;
    }
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    bearer == Some(state.token.as_str()) || query_token == Some(state.token.as_str())
}

async fn get_blob(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<BlobQuery>,
    headers: HeaderMap,
) -> Response {
    if !blob_allowed(&state, &headers, query.access_token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match state.service.blob(&id) {
        Ok(Some((path, mime))) => match std::fs::read(&path) {
            Ok(bytes) => {
                let ctype = mime.unwrap_or_else(|| "application/octet-stream".into());
                ([(header::CONTENT_TYPE, ctype)], bytes).into_response()
            }
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        },
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
#[derive(Deserialize)]
struct DebugInbound {
    #[serde(default)]
    account: Option<String>,
    #[serde(flatten)]
    envelope: Envelope,
}

async fn debug_inbound(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<DebugInbound>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if token != Some(state.token.as_str()) {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".into()));
    }
    let mut env = body.envelope;
    if let Some(account) = body.account {
        env.account = account;
    }
    state
        .service
        .ingest(Ingress::Message(env))
        .await
        .map_err(map_err)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

fn map_err(e: ChannelError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}

pub async fn serve(bind: &str, state: AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("channel server on {bind}");
    // MCP Streamable HTTP keeps SSE open; graceful drain never finishes.
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
