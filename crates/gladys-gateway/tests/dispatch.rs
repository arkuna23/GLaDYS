use std::sync::Arc;
use std::time::Duration;

use gladys_gateway::agent::FakeBackend;
use gladys_gateway::config::DreamConfig;
use gladys_gateway::daemon::{CommandReg, HandlerReg, RecDaemon, Registry};
use gladys_gateway::dispatch::Dispatch;
use gladys_gateway::io::{RecChannel, RecMemory};
use gladys_gateway::policy::Policy;
use gladys_gateway::store::Store;
use gladys_gateway::types::{
    Actor, Conversation, ConversationKind, Envelope, ListMode, MentionTarget, MemoryItem, Pack,
    Part, ReplyTo,
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
        Duration::ZERO,
        fake.clone(),
        ch.clone(),
        mem.clone(),
        Store::memory().unwrap(),
        "en",
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
        reply_to: None,
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
async fn group_reply_to_bot_is_direct() {
    let (d, fake, _, _) = harness("ok", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    let mut e = env(ConversationKind::Group, "1", "u", "followup", None);
    e.reply_to = Some(ReplyTo {
        platform_id: Some("42".into()),
        sender: Some("bot".into()),
    });
    d.handle(e).await.unwrap();
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

fn mem_item(id: &str, text: &str) -> MemoryItem {
    MemoryItem {
        id: id.into(),
        text: text.into(),
        ts: 1,
    }
}

async fn at_bot(d: &Dispatch, text: &str) {
    d.handle(env(
        ConversationKind::Group,
        "1",
        "u",
        text,
        Some("bot"),
    ))
    .await
    .unwrap();
    wait(Duration::from_millis(10)).await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn memory_full_once_then_global_delta() {
    let (d, fake, _, mem) = harness("ok", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    *mem.pack.lock().await = Pack {
        global: vec![mem_item("g1", "be brief")],
        conversation: vec![mem_item("c1", "group rule")],
        person: vec![],
    };
    at_bot(&d, "hey").await;
    {
        let p = &fake.prompts.lock().await[0];
        assert_eq!(p.pack.conversation.len(), 1);
        assert_eq!(p.pack.global.len(), 1);
        assert!(p.pack_delta.is_empty());
    }
    at_bot(&d, "again").await;
    {
        let p = &fake.prompts.lock().await[1];
        assert!(p.pack.conversation.is_empty());
        assert!(p.pack.global.is_empty());
        assert!(p.pack_delta.is_empty());
    }
    mem.pack.lock().await.global[0].text = "be shorter".into();
    at_bot(&d, "third").await;
    let p = &fake.prompts.lock().await[2];
    assert!(p.pack.conversation.is_empty());
    assert_eq!(p.pack_delta.changed.len(), 1);
    assert_eq!(p.pack_delta.changed[0].1.text, "be shorter");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn memory_full_again_after_new() {
    let (d, fake, _, mem) = harness("ok", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    *mem.pack.lock().await = Pack {
        global: vec![mem_item("g1", "be brief")],
        conversation: vec![mem_item("c1", "group rule")],
        person: vec![],
    };
    at_bot(&d, "hey").await;
    d.handle(env(
        ConversationKind::Group,
        "1",
        "owner",
        "/new",
        Some("bot"),
    ))
    .await
    .unwrap();
    at_bot(&d, "after new").await;
    let p = fake.prompts.lock().await;
    assert_eq!(p.len(), 2);
    assert_eq!(p[1].pack.conversation.len(), 1);
    assert!(p[1].pack_delta.is_empty());
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
    d.handle(env(ConversationKind::Dm, "u1", "u1", "three", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    assert!(fake.steers.lock().await.is_empty());
    let _ = hold.send(());
    wait(Duration::from_millis(1)).await;
    assert_eq!(fake.steers.lock().await.len(), 1);
    assert!(fake.steers.lock().await[0].1.contains("two"));
    assert!(fake.steers.lock().await[0].1.contains("three"));
}


fn harness_after(
    reply: &str,
    debounce: Duration,
    idle: Duration,
    after: Duration,
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
        after,
        fake.clone(),
        ch.clone(),
        mem.clone(),
        Store::memory().unwrap(),
        "en",
    );
    (d, fake, ch, mem)
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn after_run_hot_window_uses_debounce() {
    let (d, fake, _, _) = harness_after(
        "ok",
        Duration::from_millis(10),
        Duration::from_secs(30),
        Duration::from_secs(10),
    );
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
    d.handle(env(ConversationKind::Group, "1", "u", "noise", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 2);
    assert!(!fake.prompts.lock().await[1].idle);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn after_run_hot_window_expires_to_idle() {
    let (d, fake, _, _) = harness_after(
        "ok",
        Duration::from_millis(10),
        Duration::from_secs(30),
        Duration::from_secs(10),
    );
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
    wait(Duration::from_secs(10)).await;
    d.handle(env(ConversationKind::Group, "1", "u", "noise", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    wait(Duration::from_secs(30)).await;
    assert_eq!(fake.prompts.lock().await.len(), 2);
    assert!(fake.prompts.lock().await[1].idle);
}


#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn two_chats_prompt_in_parallel() {
    let (d, fake, _, _) = harness("ok", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    let hold = fake.set_hold().await;
    d.handle(env(ConversationKind::Dm, "u1", "u1", "a", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    d.handle(env(ConversationKind::Dm, "u2", "u2", "b", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 2);
    let _ = hold.send(());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn daemon_direct() {
    let (d, fake, _, _) = harness("job", Duration::from_millis(10), Duration::from_secs(30));
    let env = env(ConversationKind::Group, "1", "daemon", "wake", None);
    d.handle_daemon(env).await.unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    assert!(!fake.prompts.lock().await[0].idle);
}


#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn dream_interval_fires() {
    let (d, fake, _, _) = harness("ok", Duration::from_millis(10), Duration::from_secs(30));
    d.configure_dream(DreamConfig {
        mode: "interval".into(),
        cron: None,
        interval_secs: 5,
    })
    .await;
    d.tick_dream().await;
    assert!(fake.prompts.lock().await.is_empty());
    wait(Duration::from_secs(5)).await;
    d.tick_dream().await;
    let p = fake.prompts.lock().await;
    assert_eq!(p.len(), 1);
    assert!(p[0].dream);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn dream_interval_waits_for_agent() {
    let (d, fake, _, _) = harness("ok", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    d.configure_dream(DreamConfig {
        mode: "interval".into(),
        cron: None,
        interval_secs: 5,
    })
    .await;
    let hold = fake.set_hold().await;
    d.handle(env(ConversationKind::Dm, "u1", "u1", "hi", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    wait(Duration::from_secs(5)).await;
    d.tick_dream().await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    let _ = hold.send(());
    wait(Duration::from_millis(1)).await;
    d.tick_dream().await;
    let p = fake.prompts.lock().await;
    assert_eq!(p.len(), 2);
    assert!(p[1].dream);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn dream_at_waits_until_idle() {
    let (d, fake, _, _) = harness("ok", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    d.configure_dream(DreamConfig {
        mode: "at".into(),
        cron: None,
        interval_secs: 3600,
    })
    .await;
    let hold = fake.set_hold().await;
    d.handle(env(ConversationKind::Dm, "u1", "u1", "hi", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    d.set_dream_due().await;
    d.tick_dream().await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    let _ = hold.send(());
    wait(Duration::from_millis(1)).await;
    d.tick_dream().await;
    let p = fake.prompts.lock().await;
    assert_eq!(p.len(), 2);
    assert!(p[1].dream);
}


#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn custom_command_swallows_and_owner_level() {
    let (d, fake, _, _) = harness("x", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    let rec = std::sync::Arc::new(RecDaemon::default());
    d.set_daemon(rec.clone()).await;
    d.set_registry(Registry {
        commands: vec![
            CommandReg {
                name: "ping".into(),
                level: "user".into(),
                docs: "pong".into(),
            },
            CommandReg {
                name: "op".into(),
                level: "owner".into(),
                docs: String::new(),
            },
        ],
        handlers: vec![],
    })
    .await;

    d.handle(env(ConversationKind::Dm, "u1", "u1", "/ping a", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert!(fake.prompts.lock().await.is_empty());
    assert_eq!(rec.runs.lock().await.len(), 1);
    assert_eq!(rec.runs.lock().await[0].name, "ping");

    d.handle(env(ConversationKind::Dm, "u1", "u1", "/op", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(rec.runs.lock().await.len(), 1);
    assert_eq!(fake.prompts.lock().await.len(), 1);

    d.handle(env(
        ConversationKind::Group,
        "1",
        "u",
        "/ping",
        None,
    ))
    .await
    .unwrap();
    assert_eq!(rec.runs.lock().await.len(), 1);

    d.handle(env(
        ConversationKind::Group,
        "1",
        "u",
        "/ping",
        Some("bot"),
    ))
    .await
        .unwrap();
    wait(Duration::from_millis(1)).await;
    assert_eq!(rec.runs.lock().await.len(), 2);

    d.handle(env(ConversationKind::Dm, "u1", "u1", "/nope", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 2);
}


#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn help_lists_new_and_custom() {
    let (d, fake, ch, _) = harness("x", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    d.set_registry(Registry {
        commands: vec![
            CommandReg {
                name: "ping".into(),
                level: "user".into(),
                docs: "pong\nmore".into(),
            },
            CommandReg {
                name: "op".into(),
                level: "owner".into(),
                docs: String::new(),
            },
        ],
        handlers: vec![],
    })
    .await;
    d.handle(env(ConversationKind::Dm, "u1", "u1", "/help", None))
        .await
        .unwrap();
    assert!(fake.prompts.lock().await.is_empty());
    let sent = ch.sent.lock().await;
    let Part::Text { text } = &sent[0].2[0] else {
        panic!("help text");
    };
    assert_eq!(
        text,
        "Commands:\n/help — list commands\n/new — reset session (owners)\n/ping — pong\n/op [owner]\n"
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn inbound_handler_skips_new_and_commands() {
    let (d, fake, _, _) = harness("ok", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    let rec = std::sync::Arc::new(RecDaemon::default());
    d.set_daemon(rec.clone()).await;
    d.set_registry(Registry {
        commands: vec![CommandReg {
            name: "ping".into(),
            level: "user".into(),
            docs: String::new(),
        }],
        handlers: vec![HandlerReg {
            name: "onmsg".into(),
            event: "message.inbound".into(),
            account: None,
            kind: None,
            peer: None,
        }],
    })
    .await;

    d.handle(env(ConversationKind::Dm, "u1", "u1", "hello", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    assert_eq!(rec.runs.lock().await.len(), 1);
    assert_eq!(rec.runs.lock().await[0].kind, "handler");

    d.handle(env(ConversationKind::Dm, "u1", "u1", "/ping", None))
        .await
        .unwrap();
    wait(Duration::from_millis(1)).await;
    assert_eq!(rec.runs.lock().await.len(), 2);
    assert_eq!(rec.runs.lock().await[1].kind, "command");

    d.handle(env(ConversationKind::Dm, "owner", "owner", "/new", None))
        .await
        .unwrap();
    assert_eq!(rec.runs.lock().await.len(), 2);
    assert_eq!(fake.resets.lock().await.len(), 1);
}


#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn prompt_wait_returns_text() {
    let (d, fake, _, _) = harness("done", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    let (_id, text) = d
        .prompt_wait(env(ConversationKind::Dm, "u1", "daemon", "sum", None))
        .await
        .unwrap();
    assert_eq!(text, "done");
    assert_eq!(fake.prompts.lock().await.len(), 1);
    assert!(!fake.prompts.lock().await[0].idle);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn prompt_wait_after_running_fire() {
    let (d, fake, _, _) = harness("done", Duration::from_millis(10), Duration::from_secs(30));
    d.set_self_id("main".into(), "bot".into()).await;
    let hold = fake.set_hold().await;
    d.handle(env(ConversationKind::Dm, "u1", "u1", "one", None))
        .await
        .unwrap();
    wait(Duration::from_millis(10)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    let d2 = d.clone();
    let task = tokio::spawn(async move {
        d2.prompt_wait(env(ConversationKind::Dm, "u1", "daemon", "two", None))
            .await
    });
    wait(Duration::from_millis(50)).await;
    assert_eq!(fake.prompts.lock().await.len(), 1);
    let _ = hold.send(());
    wait(Duration::from_millis(50)).await;
    let (_id, text) = task.await.unwrap().unwrap();
    assert_eq!(text, "done");
    assert_eq!(fake.prompts.lock().await.len(), 2);
}
