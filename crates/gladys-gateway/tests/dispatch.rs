use std::sync::Arc;
use std::time::Duration;

use gladys_gateway::agent::FakeBackend;
use gladys_gateway::dispatch::Dispatch;
use gladys_gateway::io::{RecChannel, RecMemory};
use gladys_gateway::policy::Policy;
use gladys_gateway::store::Store;
use gladys_gateway::types::{
    Actor, Conversation, ConversationKind, Envelope, ListMode, MentionTarget, Part,
};

fn harness(
    reply: &str,
    debounce: Duration,
    idle: Duration,
) -> (Dispatch, Arc<FakeBackend>, Arc<RecChannel>, Arc<RecMemory>) {
    let fake = Arc::new(FakeBackend::new(reply));
    let ch = Arc::new(RecChannel::default());
    let mem = Arc::new(RecMemory::default());
    let policy = Policy {
        group_mode: ListMode::Whitelist,
        dm_mode: ListMode::Blacklist,
        groups: vec!["onebot:group:1".into()],
        dms: vec!["onebot:dm:blocked".into()],
        owners: vec!["onebot:owner".into()],
    };
    let d = Dispatch::new(
        policy,
        debounce,
        idle,
        fake.clone(),
        ch.clone(),
        mem.clone(),
        Store::memory().unwrap(),
    );
    (d, fake, ch, mem)
}

fn env(
    kind: ConversationKind,
    peer: &str,
    sender: &str,
    text: &str,
    mention: Option<&str>,
) -> Envelope {
    let mut parts = Vec::new();
    if let Some(id) = mention {
        parts.push(Part::Mention {
            target: MentionTarget::User,
            id: Some(id.into()),
        });
    }
    parts.push(Part::Text { text: text.into() });
    Envelope {
        id: "m".into(),
        channel: "onebot".into(),
        account: "main".into(),
        conversation: Conversation {
            kind,
            peer: peer.into(),
        },
        direction: "in".into(),
        sender: Actor {
            id: sender.into(),
            name: None,
        },
        parts,
    }
}

async fn wait(d: Duration) {
    tokio::task::yield_now().await;
    tokio::time::sleep(d).await;
    tokio::task::yield_now().await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn policy_drops() {
    let (d, fake, _, _) = harness("x", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    d.handle(env(ConversationKind::Group, "nope", "u", "hi", None))
        .await
        .unwrap();
    d.handle(env(ConversationKind::Dm, "blocked", "u", "hi", None))
        .await
        .unwrap();
    wait(Duration::from_secs(60)).await;
    assert!(fake.prompts.lock().await.is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn group_idle_vs_mention() {
    let (d, fake, _, mem) = harness("ok", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    d.handle(env(ConversationKind::Group, "1", "u", "noise", None))
        .await
        .unwrap();
    wait(Duration::from_secs(1)).await;
    assert!(fake.prompts.lock().await.is_empty());
    wait(Duration::from_secs(30)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    assert!(fake.prompts.lock().await[0].idle);
    let call = &mem.calls.lock().await[0];
    assert!(call.3.is_none());

    let (d, fake, _, _) = harness("ok", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    d.handle(env(
        ConversationKind::Group,
        "1",
        "u",
        "hey",
        Some("bot"),
    ))
    .await
    .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    assert!(!fake.prompts.lock().await[0].idle);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn dm_direct_uses_person_pack() {
    let (d, fake, _, mem) = harness("hi", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    d.handle(env(ConversationKind::Dm, "u1", "u1", "hello", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    assert!(!fake.prompts.lock().await[0].idle);
    assert_eq!(mem.calls.lock().await[0].3.as_deref(), Some("u1"));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn new_command_owner_only() {
    let (d, fake, ch, _) = harness("x", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    d.handle(env(ConversationKind::Dm, "u1", "u1", "/new", None))
        .await
        .unwrap();
    assert!(fake.resets.lock().await.is_empty());

    d.handle(env(ConversationKind::Dm, "owner", "owner", "/new", None))
        .await
        .unwrap();
    assert_eq!(fake.resets.lock().await.len(), 1);
    assert_eq!(ch.sent.lock().await.len(), 1);

    d.handle(env(
        ConversationKind::Group,
        "1",
        "owner",
        "/new",
        None,
    ))
    .await
    .unwrap();
    assert_eq!(fake.resets.lock().await.len(), 1);

    d.handle(env(
        ConversationKind::Group,
        "1",
        "owner",
        "/new",
        Some("bot"),
    ))
    .await
    .unwrap();
    assert_eq!(fake.resets.lock().await.len(), 2);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn steer_while_running() {
    let (d, fake, _, _) = harness("done", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    let hold = fake.set_hold().await;
    d.handle(env(ConversationKind::Dm, "u1", "u1", "one", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    d.handle(env(ConversationKind::Dm, "u1", "u1", "two", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    assert_eq!(fake.steers.lock().await.len(), 1);
    let _ = hold.send(());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn scheduler_direct() {
    let (d, fake, _, _) = harness("job", Duration::from_millis(10), Duration::from_secs(30));
    let env = env(ConversationKind::Group, "1", "scheduler", "wake", None);
    d.handle_scheduler(env).await.unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    assert!(!fake.prompts.lock().await[0].idle);
}
