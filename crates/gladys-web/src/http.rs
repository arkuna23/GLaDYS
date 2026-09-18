use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub memory_url: String,
    pub memory_token: String,
    pub http: reqwest::Client,
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/memories", any(proxy))
        .route("/api/memories/{id}", any(proxy))
        .route("/api/scopes", any(proxy))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    Router::new()
        .route("/health", get(health))
        .route("/api/login", post(login))
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

#[derive(Deserialize)]
struct LoginBody {
    token: String,
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<Json<Value>, StatusCode> {
    if body.token == state.token {
        Ok(Json(serde_json::json!({"ok": true})))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn map_path(path: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix("/api/memories") {
        return Some(format!("/v1/memories{rest}"));
    }
    if path == "/api/scopes" {
        return Some("/v1/scopes".into());
    }
    None
}

async fn proxy(State(state): State<AppState>, req: Request) -> Response {
    let Some(mapped) = map_path(req.uri().path()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let url = format!(
        "{}{mapped}{query}",
        state.memory_url.trim_end_matches('/')
    );
    let method = reqwest::Method::from_bytes(req.method().as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);
    let content_type = req.headers().get(header::CONTENT_TYPE).cloned();
    let body = match to_bytes(req.into_body(), 2 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let mut b = state
        .http
        .request(method, url)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", state.memory_token),
        );
    if let Some(ct) = content_type {
        b = b.header(header::CONTENT_TYPE, ct);
    }
    let resp = match b.body(body).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("memory proxy: {e}");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let ct = resp.headers().get(header::CONTENT_TYPE).cloned();
    let bytes = resp.bytes().await.unwrap_or_default();
    let mut out = Response::new(Body::from(bytes));
    *out.status_mut() = status;
    if let Some(ct) = ct {
        out.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    out
}

pub async fn serve(bind: &str, state: AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("web server on {bind}");
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
