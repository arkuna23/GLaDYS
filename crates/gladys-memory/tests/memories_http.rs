use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gladys_memory::http::{router, AppState};
use gladys_memory::service::{Service, WriteParams};
use gladys_memory::store::Store;
use gladys_memory::types::{Conversation, ConversationKind, Layer, Memory, Scopes};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn app() -> axum::Router {
    let svc = Service::new(Store::memory().unwrap(), 20, 4000);
    svc.write(WriteParams {
        layer: Layer::Global,
        text: "be brief".into(),
        channel: None,
        conversation: None,
        person: None,
    })
    .unwrap();
    svc.write(WriteParams {
        layer: Layer::Conversation,
        text: "group rule".into(),
        channel: Some("onebot".into()),
        conversation: Some(Conversation {
            kind: ConversationKind::Group,
            peer: "1".into(),
        }),
        person: None,
    })
    .unwrap();
    router(AppState {
        service: Arc::new(svc),
        token: "t".into(),
    })
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

#[tokio::test]
async fn list_patch_scopes() {
    let app = app();
    let (status, _) = send(
        &app,
        Request::builder()
            .uri("/v1/memories")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = send(
        &app,
        Request::builder()
            .uri("/v1/memories")
            .header("Authorization", "Bearer t")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let listed: Vec<Memory> = serde_json::from_slice(&body).unwrap();
    assert_eq!(listed.len(), 2);

    let conv = listed
        .iter()
        .find(|m| m.layer == Layer::Conversation)
        .unwrap();
    let (status, body) = send(
        &app,
        Request::builder()
            .method("PATCH")
            .uri(format!("/v1/memories/{}", conv.id))
            .header("Authorization", "Bearer t")
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"text":"updated rule"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let patched: Memory = serde_json::from_slice(&body).unwrap();
    assert_eq!(patched.text, "updated rule");
    assert_eq!(patched.id, conv.id);

    let (status, body) = send(
        &app,
        Request::builder()
            .uri("/v1/scopes")
            .header("Authorization", "Bearer t")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let scopes: Scopes = serde_json::from_slice(&body).unwrap();
    assert_eq!(scopes.channels, vec!["onebot".to_string()]);
    assert_eq!(scopes.conversations.len(), 1);
    assert_eq!(scopes.conversations[0].peer, "1");
}
