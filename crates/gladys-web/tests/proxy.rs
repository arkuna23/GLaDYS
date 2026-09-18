use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use gladys_web::http::{router, AppState};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn web_app(memory_url: &str) -> Router {
    router(AppState {
        token: "web".into(),
        memory_url: memory_url.into(),
        memory_token: "mem".into(),
        http: reqwest::Client::new(),
    })
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

#[tokio::test]
async fn login_ok_and_reject() {
    let app = web_app("http://127.0.0.1:9");
    let (status, body) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/login")
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"token":"web"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.windows(b"true".len()).any(|w| w == b"true"));

    let (status, _) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/login")
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"token":"nope"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn memories_proxy_uses_memory_token() {
    let seen = Arc::new(Mutex::new(None::<String>));
    let flag = seen.clone();
    let mock = Router::new().route(
        "/v1/memories",
        get(move |headers: HeaderMap| {
            let flag = flag.clone();
            async move {
                *flag.lock().unwrap() = headers
                    .get(header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                Json(serde_json::json!([]))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, mock).await.unwrap();
    });

    let app = web_app(&format!("http://{addr}"));
    let (status, _) = send(
        &app,
        Request::builder()
            .uri("/api/memories")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = send(
        &app,
        Request::builder()
            .uri("/api/memories")
            .header("Authorization", "Bearer web")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"[]");
    assert_eq!(seen.lock().unwrap().as_deref(), Some("Bearer mem"));
}
