use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use gladys_channel::adapter::loopback::LoopbackAdapter;
use gladys_channel::adapter::Adapter;
use gladys_channel::config::AccountConfig;
use gladys_channel::http::{router, AppState};
use gladys_channel::message::{Conversation, Direction, Envelope, Ingress, Part};
use gladys_channel::service::Service;
use gladys_channel::store::Store;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

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
        self_id: Some("bot".into()),
    }
}

#[tokio::test]
async fn hello_inbound_and_send() {
    let dir = {
        let p = std::env::temp_dir().join("gladys-ws-test");
        std::fs::create_dir_all(&p).expect("temp");
        p
    };
    let store = Store::memory(&dir).expect("store");
    let lb = Arc::new(LoopbackAdapter::new(&loopback_cfg()));
    let mut adapters: HashMap<String, Arc<dyn Adapter>> = HashMap::new();
    adapters.insert("lb".into(), lb);
    let service = Arc::new(Service::new(store, adapters));
    let app = router(AppState {
        service: service.clone(),
        token: "test-token".into(),
        debug: true,
        loopback_blobs: true,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    let url = format!("ws://{addr}/v1/gateway");
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws connect");
    ws.send(Message::Text(
        json!({"v":1,"type":"hello","payload":{"role":"gateway","token":"test-token"}})
            .to_string()
            .into(),
    ))
    .await
    .expect("hello");

    let hello = recv_json(&mut ws).await;
    assert_eq!(hello["type"], "hello_ok");
    let _health = recv_json(&mut ws).await;

    service
        .ingest(Ingress::Message(Envelope {
            id: Envelope::new_id(),
            channel: "loopback".into(),
            account: "lb".into(),
            conversation: Conversation::dm("9"),
            direction: Direction::In,
            sender: gladys_channel::message::Actor {
                id: "9".into(),
                name: None,
            },
            ts: 1,
            platform_id: Some("in-1".into()),
            reply_to: None,
            parts: vec![Part::text("ping")],
            native: None,
            recalled: false,
        }))
        .await
        .expect("ingest");

    let inbound = recv_json(&mut ws).await;
    assert_eq!(inbound["event"], "message.inbound");
    assert_eq!(inbound["payload"]["parts"][0]["text"], "ping");

    ws.send(Message::Text(
        json!({
            "v":1,
            "type":"req",
            "id":"1",
            "method":"send",
            "params":{
                "account":"lb",
                "conversation":{"kind":"dm","peer":"9"},
                "parts":[{"type":"text","text":"pong"}]
            }
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send");

    let mut got_res = false;
    let mut got_out = false;
    for _ in 0..6 {
        let frame = recv_json(&mut ws).await;
        if frame["type"] == "res" && frame["id"] == "1" {
            assert_eq!(frame["ok"], true);
            got_res = true;
        }
        if frame["event"] == "message.outbound" {
            got_out = true;
        }
        if got_res && got_out {
            break;
        }
    }
    assert!(got_res, "missing send res");
    assert!(got_out, "missing outbound event");
}

async fn recv_json<S>(ws: &mut S) -> Value
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        return serde_json::from_str(t.as_str()).expect("json");
                    }
                    Some(Ok(_)) => continue,
                    other => panic!("ws closed: {other:?}"),
                }
            }
            _ = &mut deadline => panic!("timeout waiting for ws frame"),
        }
    }
}
