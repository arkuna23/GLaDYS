use std::sync::Arc;
use std::time::Duration;

use axum::routing::post;
use axum::{Json, Router};
use gladys_daemon::app::App;
use gladys_daemon::channel_mcp::RecTools;
use gladys_daemon::client::GatewayClient;
use gladys_daemon::http::{router, AppState};
use gladys_daemon::runner::{exec_script, Conversation, RunRequest};
use gladys_daemon::store::{Command, Store};
use rusqlite::Connection;
use serde_json::{json, Value};
use tokio::sync::Mutex;

fn req() -> RunRequest {
    RunRequest {
        kind: "command".into(),
        name: "ping".into(),
        account: "qq-main".into(),
        channel: "onebot".into(),
        conversation: Conversation {
            kind: "group".into(),
            peer: "1".into(),
        },
        sender: Default::default(),
        text: "/ping".into(),
        argv: vec![],
        event: String::new(),
        payload: Value::Null,
        lang: "en".into(),
    }
}

async fn mock_gateway(prompt_text: &'static str) -> GatewayClient {
    let hits = Arc::new(Mutex::new(Vec::new()));
    let h2 = hits.clone();
    let app = Router::new()
        .route(
            "/v1/daemon/prompt",
            post(move |Json(_body): Json<Value>| async move {
                Json(json!({"ok": true, "id": "1", "text": prompt_text}))
            }),
        )
        .route(
            "/v1/daemon/jobs",
            post({
                let h2 = h2.clone();
                move |Json(body): Json<Value>| {
                    let h2 = h2.clone();
                    async move {
                        h2.lock().await.push(body.clone());
                        Json(json!({"id": "job1"}))
                    }
                }
            }),
        )
        .with_state(());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    GatewayClient::new(format!("http://{addr}"), "t".into())
}

#[test]
fn store_script_roundtrip() {
    let store = Store::memory().unwrap();
    store
        .put_command(&Command {
            name: "ping".into(),
            level: "user".into(),
            script: "channel.send{parts={{type=\"text\", text=\"pong\"}}}".into(),
            docs: "pong".into(),
        })
        .unwrap();
    let c = store.get_command("ping").unwrap();
    assert_eq!(c.script, "channel.send{parts={{type=\"text\", text=\"pong\"}}}");
    let text = store.docs(Some("ping")).unwrap();
    assert!(text.contains("/ping"));
    assert!(text.contains("user"));
    assert!(text.contains("pong"));
    assert!(text.contains("agent:wait"));
    assert!(!text.contains("tool json"));
}

#[test]
fn migrate_legacy_command_table() {
    let path = std::env::temp_dir().join(format!("gladys-lua-mig-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE commands (
               name TEXT PRIMARY KEY,
               level TEXT NOT NULL,
               command TEXT NOT NULL,
               args TEXT NOT NULL DEFAULT '[]',
               docs TEXT NOT NULL DEFAULT ''
             );
             INSERT INTO commands(name, level, command, docs) VALUES('old','user','echo','hi');",
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let c = store.get_command("old").unwrap();
    assert_eq!(c.level, "user");
    assert_eq!(c.docs, "hi");
    assert_eq!(c.script, "");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn lua_channel_send_fills_account() {
    let tools = Arc::new(RecTools::default());
    let mem = Arc::new(RecTools::default());
    let gw = mock_gateway("").await;
    exec_script(
        r#"channel.send{parts={{type="text", text="hi"}}}"#,
        &req(),
        tools.clone(),
        mem,
        gw,
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    let calls = tools.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "channel");
    assert_eq!(calls[0].1["op"], "send");
    assert_eq!(calls[0].1["account"], "qq-main");
}


#[tokio::test]
async fn lua_channel_call_uses_native_tool() {
    let tools = Arc::new(RecTools::default());
    let mem = Arc::new(RecTools::default());
    let gw = mock_gateway("").await;
    exec_script(
        r#"channel.call{op="get_group_info", params={}}"#,
        &req(),
        tools.clone(),
        mem,
        gw,
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    let calls = tools.calls.lock().await;
    assert_eq!(calls[0].0, "channel_call");
    assert_eq!(calls[0].1["op"], "get_group_info");
}

#[tokio::test]
async fn lua_agent_prompt_wait() {
    let tools = Arc::new(RecTools::default());
    let mem = Arc::new(RecTools::default());
    let gw = mock_gateway("sum-ok").await;
    exec_script(
        r#"
        agent:prompt("summarize")
        local t = agent:wait()
        channel.send{parts={{type="text", text=t}}}
        "#,
        &req(),
        tools.clone(),
        mem,
        gw,
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    let calls = tools.calls.lock().await;
    assert_eq!(calls[0].1["parts"][0]["text"], "sum-ok");
}

#[tokio::test]
async fn lua_memory_and_job() {
    let ch = Arc::new(RecTools::default());
    let mem = Arc::new(RecTools::default());
    let gw = mock_gateway("").await;
    exec_script(
        r#"
        memory.write{layer="conversation", text="note"}
        job.delay{after_secs=60, text="later"}
        "#,
        &req(),
        ch,
        mem.clone(),
        gw,
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    let calls = mem.calls.lock().await;
    assert_eq!(calls[0].0, "memory");
    assert_eq!(calls[0].1["op"], "write");
    assert_eq!(calls[0].1["text"], "note");
    assert_eq!(calls[0].1["channel"], "onebot");
}

#[tokio::test]
async fn lua_agent_wait_without_prompt_errors() {
    let ch = Arc::new(RecTools::default());
    let mem = Arc::new(RecTools::default());
    let gw = mock_gateway("").await;
    let err = exec_script("agent:wait()", &req(), ch, mem, gw, Duration::from_secs(2))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no prompt"));
}

#[tokio::test]
async fn lua_double_prompt_errors() {
    let ch = Arc::new(RecTools::default());
    let mem = Arc::new(RecTools::default());
    let gw = mock_gateway("x").await;
    let err = exec_script(
        r#"
        agent:prompt("a")
        agent:prompt("b")
        "#,
        &req(),
        ch,
        mem,
        gw,
        Duration::from_secs(2),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("already running"));
}

#[tokio::test]
async fn lua_os_ok_require_blocked() {
    let ch = Arc::new(RecTools::default());
    let mem = Arc::new(RecTools::default());
    let gw = mock_gateway("").await;
    exec_script("assert(os.clock() >= 0)", &req(), ch.clone(), mem.clone(), gw.clone(), Duration::from_secs(2))
        .await
        .unwrap();
    exec_script("require('x')", &req(), ch, mem, gw, Duration::from_secs(2))
        .await
        .unwrap_err();
}


#[tokio::test]
async fn lua_check_ok_and_syntax() {
    assert!(gladys_daemon::lua::check(
        r#"channel.send{parts={{type="text", text="x"}}}"#,
        Duration::from_secs(2),
    )
    .await
    .is_ok());
    assert!(gladys_daemon::lua::check("function(", Duration::from_secs(2))
        .await
        .is_err());
}

#[tokio::test]
async fn upload_command_http() {
    let store = Store::memory().unwrap();
    let app = App {
        store: store.clone(),
        gateway: Arc::new(GatewayClient::new("http://127.0.0.1:9".into(), "t".into())),
        channel: Arc::new(RecTools::default()),
        memory: Arc::new(RecTools::default()),
    };
    let state = AppState {
        app,
        token: "t".into(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router(state)).await.unwrap();
    });
    let client = reqwest::Client::new();
    let resp = client
        .put(format!("http://{addr}/v1/scripts/ping"))
        .header("Authorization", "Bearer t")
        .header("X-Gladys-Type", "command")
        .header("X-Gladys-Level", "user")
        .header("X-Gladys-Docs", "pong")
        .body(r#"channel.send{parts={{type="text", text="pong"}}}"#)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let c = store.get_command("ping").unwrap();
    assert!(c.script.contains("pong"));
    assert_eq!(c.docs, "pong");
}


#[tokio::test]
async fn upload_missing_type_is_bad_request() {
    let store = Store::memory().unwrap();
    let app = App {
        store: store.clone(),
        gateway: Arc::new(GatewayClient::new("http://127.0.0.1:9".into(), "t".into())),
        channel: Arc::new(RecTools::default()),
        memory: Arc::new(RecTools::default()),
    };
    let state = AppState {
        app,
        token: "t".into(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router(state)).await.unwrap();
    });
    let client = reqwest::Client::new();
    let resp = client
        .put(format!("http://{addr}/v1/scripts/ping"))
        .header("Authorization", "Bearer t")
        .body("print(1)")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}
