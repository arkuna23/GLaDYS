use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gladys_gateway::agent::FakeBackend;
use gladys_gateway::dispatch::Dispatch;
use gladys_gateway::http::{router, AppState};
use gladys_gateway::io::{RecChannel, RecMemory};
use gladys_gateway::jobs::{JobSpec, Jobs};
use gladys_gateway::policy::Policy;
use gladys_gateway::store::Store;
use gladys_gateway::types::{Conversation, ConversationKind, ListMode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn harness() -> (Jobs, Arc<FakeBackend>, Dispatch) {
    let fake = Arc::new(FakeBackend::new("ok"));
    let ch = Arc::new(RecChannel::default());
    let mem = Arc::new(RecMemory::default());
    let store = Store::memory().unwrap();
    let dispatch = Dispatch::new(
        Policy {
            group_mode: ListMode::Whitelist,
            dm_mode: ListMode::Blacklist,
            groups: vec!["onebot:group:1".into()],
            dms: vec![],
            owners: vec![],
        },
        Duration::from_millis(10),
        Duration::from_secs(30),
        fake.clone(),
        ch,
        mem,
        store.clone(),
        "en",
    );
    (Jobs::new(store, dispatch.clone()), fake, dispatch)
}

fn spec(kind: &str) -> JobSpec {
    JobSpec {
        kind: kind.into(),
        account: "main".into(),
        channel: "onebot".into(),
        conversation: Conversation {
            kind: ConversationKind::Group,
            peer: "1".into(),
        },
        text: "wake".into(),
        cron: None,
        after_secs: None,
        command: None,
        args: vec![],
    }
}

async fn wait_fire() {
    tokio::time::sleep(Duration::from_millis(30)).await;
}

#[tokio::test]
async fn jobs_http_create_list_delete() {
    let (jobs, _, dispatch) = harness();
    let app = router(AppState {
        token: "t".into(),
        dispatch,
        jobs,
    });
    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/daemon/jobs")
                .header("Authorization", "Bearer t")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "kind": "delay",
                        "account": "main",
                        "channel": "onebot",
                        "conversation": {"kind": "group", "peer": "1"},
                        "text": "later",
                        "after_secs": 60
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let created: Value =
        serde_json::from_slice(&create.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let id = created["id"].as_str().unwrap().to_string();

    let listed = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/daemon/jobs")
                .header("Authorization", "Bearer t")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&listed.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);

    let del = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/daemon/jobs/{id}"))
                .header("Authorization", "Bearer t")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::OK);
}

#[tokio::test]
async fn delay_zero_fires_and_deletes() {
    let (jobs, fake, _) = harness();
    let mut spec = spec("delay");
    spec.after_secs = Some(0);
    jobs.create(spec).await.unwrap();
    jobs.tick().await.unwrap();
    wait_fire().await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    assert!(jobs.list().unwrap().is_empty());
}

#[tokio::test]
async fn cron_reschedules() {
    let (jobs, fake, _) = harness();
    let mut spec = spec("cron");
    spec.cron = Some("* * * * * *".into());
    let job = jobs.create(spec).await.unwrap();
    let due = job.next_fire.unwrap();
    jobs.tick_at(due).await.unwrap();
    wait_fire().await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    let listed = jobs.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].last_fire.is_some());
    assert!(listed[0].next_fire.is_some());
}

#[tokio::test]
async fn stdio_rejected() {
    let (jobs, _, _) = harness();
    let mut spec = spec("stdio");
    spec.command = Some("sh".into());
    assert!(jobs.create(spec).await.is_err());
}
