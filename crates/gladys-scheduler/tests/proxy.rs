use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use gladys_scheduler::GatewayClient;
use serde_json::{json, Value};

#[derive(Clone, Default)]
struct Mock {
    jobs: Arc<Mutex<Vec<Value>>>,
}

async fn create(State(m): State<Mock>, Json(body): Json<Value>) -> Json<Value> {
    let mut job = body;
    job["id"] = json!("job1");
    m.jobs.lock().unwrap().push(job.clone());
    Json(job)
}

async fn list(State(m): State<Mock>) -> Json<Value> {
    Json(json!({ "data": m.jobs.lock().unwrap().clone() }))
}

async fn delete(State(m): State<Mock>, Path(id): Path<String>) -> Json<Value> {
    m.jobs.lock().unwrap().retain(|j| j["id"] != id);
    Json(json!({ "ok": true }))
}

#[tokio::test]
async fn client_create_list_cancel() {
    let mock = Mock::default();
    let app = Router::new()
        .route("/v1/scheduler/jobs", post(create).get(list))
        .route("/v1/scheduler/jobs/{id}", axum::routing::delete(delete))
        .with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let c = GatewayClient::new(format!("http://{addr}"), "t".into());
    let created = c
        .create(json!({
            "kind": "delay",
            "account": "main",
            "channel": "onebot",
            "conversation": {"kind": "group", "peer": "1"},
            "text": "hi",
            "after_secs": 1
        }))
        .await
        .unwrap();
    assert_eq!(created["id"], "job1");
    let listed = c.list().await.unwrap();
    assert_eq!(listed["data"].as_array().unwrap().len(), 1);
    c.cancel("job1").await.unwrap();
    let listed = c.list().await.unwrap();
    assert!(listed["data"].as_array().unwrap().is_empty());
}
