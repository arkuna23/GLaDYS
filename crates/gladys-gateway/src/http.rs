use axum::extract::{Path, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::dispatch::Dispatch;
use crate::error::GatewayError;
use crate::jobs::{JobSpec, Jobs};
use crate::store::Job;
use crate::types::{Actor, Conversation, Envelope, Part};

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub dispatch: Dispatch,
    pub jobs: Jobs,
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/v1/scheduler/trigger", post(scheduler_trigger))
        .route("/v1/scheduler/jobs", post(create_job).get(list_jobs))
        .route("/v1/scheduler/jobs/{id}", axum::routing::delete(delete_job))
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

#[derive(Deserialize)]
struct TriggerBody {
    account: String,
    channel: String,
    conversation: Conversation,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

async fn scheduler_trigger(
    State(state): State<AppState>,
    Json(body): Json<TriggerBody>,
) -> Result<Json<Value>, StatusCode> {
    let env = Envelope {
        id: String::new(),
        channel: body.channel,
        account: body.account,
        conversation: body.conversation,
        direction: "in".into(),
        sender: Actor {
            id: "scheduler".into(),
            name: Some("scheduler".into()),
        },
        parts: vec![Part::Text {
            text: body.text.unwrap_or_else(|| {
                body.payload
                    .as_ref()
                    .map(|p| p.to_string())
                    .unwrap_or_default()
            }),
        }],
    };
    match state.dispatch.handle_scheduler(env).await {
        Ok(id) => Ok(Json(serde_json::json!({"ok": true, "id": id}))),
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

async fn create_job(
    State(state): State<AppState>,
    Json(spec): Json<JobSpec>,
) -> Result<Json<Job>, StatusCode> {
    state.jobs.create(spec).await.map(Json).map_err(job_err)
}

async fn list_jobs(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let jobs = state.jobs.list().map_err(job_err)?;
    Ok(Json(serde_json::json!({"data": jobs})))
}

async fn delete_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    match state.jobs.cancel(&id).await {
        Ok(true) => Ok(Json(serde_json::json!({"ok": true}))),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(e) => Err(job_err(e)),
    }
}

fn job_err(e: GatewayError) -> StatusCode {
    match e {
        GatewayError::NotFound => StatusCode::NOT_FOUND,
        GatewayError::Invalid(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn serve(bind: &str, state: AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("gateway http on {bind}");
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
