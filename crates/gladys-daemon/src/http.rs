use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
use rmcp::transport::StreamableHttpServerConfig;

use crate::app::App;
use crate::mcp::DaemonMcp;
use crate::runner::RunRequest;
use crate::store::{Command, Handler};
#[derive(Clone)]
pub struct AppState {
    pub app: App,
    pub token: String,
}

pub fn router(state: AppState) -> Router {
    let mcp_state = state.clone();
    let mcp: StreamableHttpService<DaemonMcp, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(DaemonMcp::new(mcp_state.app.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let protected = Router::new()
        .nest_service("/mcp", mcp)
        .route("/v1/run", post(run))
        .route("/v1/scripts/check", post(check_script))
        .route("/v1/scripts/{name}", put(put_script))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    Router::new()
        .route("/health", get(health))
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

async fn run(State(state): State<AppState>, Json(req): Json<RunRequest>) -> Json<serde_json::Value> {
    let app = state.app.clone();
    tokio::spawn(async move {
        app.run(req).await;
    });
    Json(serde_json::json!({"ok": true}))
}

fn hdr(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

fn body_script(body: Bytes) -> Result<String, StatusCode> {
    let s = String::from_utf8(body.to_vec()).map_err(|_| StatusCode::BAD_REQUEST)?;
    if s.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(s)
}

async fn check_script(body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    let script = body_script(body)?;
    match crate::lua::check(&script, Duration::from_secs(5)).await {
        Ok(()) => Ok(Json(serde_json::json!({"ok": true}))),
        Err(e) => Ok(Json(serde_json::json!({"ok": false, "error": e.to_string()}))),
    }
}

async fn put_script(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let script = body_script(body)?;
    let docs = hdr(&headers, "x-gladys-docs").unwrap_or_default();
    match hdr(&headers, "x-gladys-type").as_deref() {
        Some("command") => {
            let c = Command {
                name,
                level: hdr(&headers, "x-gladys-level").unwrap_or_else(|| "user".into()),
                script,
                docs,
            };
            state.app.put_command(&c).map_err(|_| StatusCode::BAD_REQUEST)?;
            state.app.push_registry().await;
            Ok(Json(serde_json::json!({"ok": true, "name": c.name, "type": "command"})))
        }
        Some("handler") => {
            let h = Handler {
                name,
                event: hdr(&headers, "x-gladys-event").unwrap_or_else(|| "message.inbound".into()),
                account: hdr(&headers, "x-gladys-account"),
                kind: hdr(&headers, "x-gladys-kind"),
                peer: hdr(&headers, "x-gladys-peer"),
                script,
                docs,
            };
            state
                .app
                .put_handler(&h)
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            state.app.push_registry().await;
            Ok(Json(serde_json::json!({"ok": true, "name": h.name, "type": "handler"})))
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

pub async fn serve(bind: &str, state: AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("daemon mcp on {bind}");
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
