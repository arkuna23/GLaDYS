use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gladys_memory::http::{router, AppState};
use gladys_memory::service::{PackParams, Service, WriteParams};
use gladys_memory::store::Store;
use gladys_memory::types::{Conversation, ConversationKind, Layer};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn pack_http_matches_service() {
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
    let expected = svc
        .pack(PackParams {
            channel: Some("onebot".into()),
            conversation: Some(Conversation {
                kind: ConversationKind::Group,
                peer: "1".into(),
            }),
            person: None,
        })
        .unwrap();

    let app = router(AppState {
        service: Arc::new(svc),
        token: "t".into(),
    });
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/pack?channel=onebot&kind=group&peer=1")
                .header("Authorization", "Bearer t")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let got: gladys_memory::types::Pack = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(got.global.len(), expected.global.len());
    assert_eq!(got.conversation.len(), expected.conversation.len());
    assert_eq!(got.person.len(), expected.person.len());
    assert_eq!(got.conversation[0].text, "group rule");
}
