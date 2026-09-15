use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
use rmcp::transport::StreamableHttpServerConfig;

use crate::client::GatewayClient;
use crate::mcp::SchedulerMcp;

#[derive(Clone)]
pub struct AppState {
    pub client: Arc<GatewayClient>,
    pub token: String,
}

pub fn router(state: AppState) -> Router {
    let mcp_state = state.clone();
    let mcp: StreamableHttpService<SchedulerMcp, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(SchedulerMcp::new(mcp_state.client.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let protected_mcp = Router::new().nest_service("/mcp", mcp).layer(
        middleware::from_fn_with_state(state.clone(), auth_middleware),
    );
    Router::new()
        .route("/health", get(health))
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

pub async fn serve(bind: &str, state: AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("scheduler mcp on {bind}");
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
