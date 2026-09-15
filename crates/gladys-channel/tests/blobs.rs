use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gladys_channel::adapter::loopback::LoopbackAdapter;
use gladys_channel::adapter::Adapter;
use gladys_channel::config::AccountConfig;
use gladys_channel::http::{router, AppState};
use gladys_channel::message::{Actor, Conversation, Direction, Envelope, Part};
use gladys_channel::service::Service;
use gladys_channel::store::Store;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn loopback_cfg() -> AccountConfig {
    AccountConfig {
        id: "lb".into(),
        channel: "loopback".into(),
        profile: None,
        mode: None,
        listen: None,
        ws_url: None,
        api_base_url: None,
        access_token_env: None,
        download_media: None,
    }
}

#[tokio::test]
async fn blob_url_in_history_and_http_get() {
    let dir = {
        let p = std::env::temp_dir().join("gladys-blob-test");
        std::fs::create_dir_all(&p).expect("temp");
        p
    };
    let store = Store::memory(&dir).expect("store");
    let id = store
        .save_blob(b"hello-img", Some("image/jpeg"), None)
        .unwrap();
    let url = format!("http://127.0.0.1:3920/v1/blobs/{id}");
    let env = Envelope {
        id: Envelope::new_id(),
        channel: "loopback".into(),
        account: "lb".into(),
        conversation: Conversation::dm("1"),
        direction: Direction::In,
        sender: Actor {
            id: "1".into(),
            name: None,
        },
        ts: 1,
        platform_id: Some("p-img".into()),
        reply_to: None,
        parts: vec![Part::Image {
            blob_id: Some(id.clone()),
            url: Some(url.clone()),
            mime: Some("image/jpeg".into()),
            filename: None,
        }],
        native: None,
        recalled: false,
    };
    store.insert_message(&env).unwrap();
    let hist = store
        .history("loopback", "lb", &Conversation::dm("1"), 10, None)
        .unwrap();
    match &hist[0].parts[0] {
        Part::Image { url: Some(u), blob_id, .. } => {
            assert_eq!(u, &url);
            assert_eq!(blob_id.as_deref(), Some(id.as_str()));
        }
        other => panic!("expected image part, got {other:?}"),
    }

    let lb = Arc::new(LoopbackAdapter::new(&loopback_cfg()));
    let mut adapters: HashMap<String, Arc<dyn Adapter>> = HashMap::new();
    adapters.insert("lb".into(), lb);
    let service = Arc::new(Service::new(store, adapters));
    let app = router(AppState {
        service,
        token: "test-token".into(),
        debug: false,
        loopback_blobs: true,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/blobs/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "image/jpeg"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..], b"hello-img");
}
