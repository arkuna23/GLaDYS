use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use cron::Schedule;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::agent::AgentBackend;
use crate::config::DreamConfig;
use crate::error::Result;
use crate::io::{ChannelIo, MemoryIo};
use crate::policy::{command_invocation, is_new_command, Policy};
use crate::store::Store;
use crate::types::{AgentInput, Conversation, ConversationKind, ConvKey, Envelope, Part};
use crate::daemon::{payload_from_env, run_json, Registry, SharedDaemon};

#[derive(Clone)]
pub struct Dispatch {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    policy: Policy,
    debounce: Duration,
    idle: Duration,
    backend: Arc<dyn AgentBackend>,
    channel: Arc<dyn ChannelIo>,
    memory: Arc<dyn MemoryIo>,
    store: Store,
    lang: String,
    registry: Mutex<Registry>,
    daemon: Mutex<Option<SharedDaemon>>,
    self_ids: Mutex<HashMap<String, String>>,
    convs: Mutex<HashMap<ConvKey, ConvState>>,
    dreaming: Mutex<bool>,
    dream: Mutex<DreamRuntime>,
}

struct ConvState {
    epoch: u64,
    running: bool,
    pending: Vec<Envelope>,
    steer: Vec<Envelope>,
}

struct DreamRuntime {
    cfg: DreamConfig,
    due_at: bool,
    next_cron: Option<i64>,
    last: Instant,
}

fn dream_key() -> ConvKey {
    ConvKey::new(
        "dream",
        &Conversation {
            kind: ConversationKind::Dm,
            peer: "global".into(),
        },
    )
}

impl Dispatch {
    pub fn new(
        policy: Policy,
        debounce: Duration,
        idle: Duration,
        backend: Arc<dyn AgentBackend>,
        channel: Arc<dyn ChannelIo>,
        memory: Arc<dyn MemoryIo>,
        store: Store,
        lang: impl Into<String>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                policy,
                debounce,
                idle,
                backend,
                channel,
                memory,
                store,
                lang: lang.into(),
                registry: Mutex::new(Registry::default()),
                daemon: Mutex::new(None),
                self_ids: Mutex::new(HashMap::new()),
                convs: Mutex::new(HashMap::new()),
                dreaming: Mutex::new(false),
                dream: Mutex::new(DreamRuntime {
                    cfg: DreamConfig::default(),
                    due_at: false,
                    next_cron: None,
                    last: Instant::now(),
                }),
            }),
        }
    }

    pub async fn set_self_id(&self, account: String, self_id: String) {
        self.inner.self_ids.lock().await.insert(account, self_id);
    }

    pub async fn handle(&self, env: Envelope) -> Result<()> {
        handle(self.inner.clone(), env).await
    }

    pub async fn set_daemon(&self, d: SharedDaemon) {
        *self.inner.daemon.lock().await = Some(d);
    }

    pub async fn set_registry(&self, reg: Registry) {
        *self.inner.registry.lock().await = reg;
    }

    pub async fn handle_daemon(&self, mut env: Envelope) -> Result<String> {
        let id = self.inner.store.log_scheduler(
            &env.account,
            &env.channel,
            &env.conversation,
            Some(&env.flatten_text()),
            None,
        )?;
        env.direction = "in".into();
        tracing::info!(
            account = %env.account,
            conv = %env.key().as_list_key(),
            text = %preview(&env.flatten_text()),
            "daemon trigger"
        );
        enqueue(self.inner.clone(), env, true).await?;
        Ok(id)
    }

    pub async fn prompt_wait(&self, mut env: Envelope) -> Result<(String, String)> {
        let id = self.inner.store.log_scheduler(
            &env.account,
            &env.channel,
            &env.conversation,
            Some(&env.flatten_text()),
            None,
        )?;
        env.direction = "in".into();
        tracing::info!(
            account = %env.account,
            conv = %env.key().as_list_key(),
            text = %preview(&env.flatten_text()),
            "daemon prompt"
        );
        let text = prompt_wait_run(self.inner.clone(), env).await?;
        Ok((id, text))
    }

    pub async fn handle_bus_event(&self, event: &str, payload: serde_json::Value) {
        if event == "message.inbound" {
            return;
        }
        let account = payload
            .get("account")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let (kind, peer) = payload
            .get("conversation")
            .map(|c| {
                (
                    c.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    c.get("peer").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                )
            })
            .unwrap_or_default();
        fire_handlers(
            self.inner.clone(),
            event,
            &account,
            payload.get("channel").and_then(|v| v.as_str()).unwrap_or(""),
            &kind,
            &peer,
            &crate::types::Actor {
                id: "daemon".into(),
                name: None,
            },
            "",
            payload.clone(),
        )
        .await;
    }

    pub async fn configure_dream(&self, cfg: DreamConfig) {
        let next_cron = cfg.cron.as_deref().and_then(next_cron_ts);
        *self.inner.dream.lock().await = DreamRuntime {
            cfg,
            due_at: false,
            next_cron,
            last: Instant::now(),
        };
    }

    pub fn spawn_dream_loop(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                this.tick_dream().await;
            }
        });
    }

    pub async fn set_dream_due(&self) {
        self.inner.dream.lock().await.due_at = true;
    }

    pub async fn tick_dream(&self) {
        let (mode, due_at) = {
            let mut st = self.inner.dream.lock().await;
            match st.cfg.mode.as_str() {
                "interval" => {
                    if st.last.elapsed() >= Duration::from_secs(st.cfg.interval_secs) {
                        st.due_at = true;
                    }
                }
                "at" => {
                    if let Some(ts) = st.next_cron {
                        if Utc::now().timestamp() >= ts {
                            st.due_at = true;
                            st.next_cron = st.cfg.cron.as_deref().and_then(next_cron_ts);
                        }
                    }
                }
                _ => {}
            }
            (st.cfg.mode.clone(), st.due_at)
        };
        if matches!(mode.as_str(), "interval" | "at") && due_at && !busy(&self.inner).await {
            self.inner.dream.lock().await.due_at = false;
            run_dream(&self.inner).await;
        }
    }
}

async fn handle(inner: Arc<Inner>, env: Envelope) -> Result<()> {
    if env.direction == "out" {
        return Ok(());
    }
    if let Some(me) = inner.self_ids.lock().await.get(&env.account).cloned()
        && env.sender.id == me
    {
        return Ok(());
    }
    let key = env.key();
    if !inner.policy.admit(&key) {
        return Ok(());
    }

    let me = inner.self_ids.lock().await.get(&env.account).cloned();
    let mentioned = me
        .as_deref()
        .map(|id| env.mentions_user(id))
        .unwrap_or(false);
    let reply = me
        .as_deref()
        .map(|id| env.replies_to(id))
        .unwrap_or(false);
    let dm = env.conversation.kind == ConversationKind::Dm;
    let owner = inner.policy.is_owner(&env.channel, &env.sender.id);
    let text = env.flatten_text();
    tracing::info!(
        account = %env.account,
        conv = %key.as_list_key(),
        from = %env.sender.id,
        text = %preview(&text),
        "inbound"
    );

    if is_new_command(dm, mentioned, owner, &text) {
        tracing::info!(conv = %key.as_list_key(), "session reset");
        inner.backend.reset(&key).await?;
        let _ = inner.store.drop_session(&key.as_list_key());
        inner
            .channel
            .send(
                &env.account,
                &env.conversation,
                vec![Part::Text {
                    text: cmd(&inner.lang, "session_reset").into(),
                }],
            )
            .await?;
        inner.convs.lock().await.remove(&key);
        return Ok(());
    }

    if let Some((name, _)) = command_invocation(dm, mentioned, &text)
        && name == "help"
    {
        let commands = inner.registry.lock().await.commands.clone();
        let body = format_help(&inner.lang, &commands);
        let _ = inner
            .channel
            .send(&env.account, &env.conversation, vec![Part::Text { text: body }])
            .await;
        return Ok(());
    }
    if let Some((name, argv)) = command_invocation(dm, mentioned, &text) {
        let level = inner
            .registry
            .lock()
            .await
            .command_level(&name)
            .map(str::to_string);
        if let Some(level) = level {
            if level == "user" || owner {
                spawn_run(
                    inner.clone(),
                    run_json(
                        "command",
                        &name,
                        &env.account,
                        &env.channel,
                        &env.conversation,
                        &env.sender,
                        &text,
                        argv,
                        "",
                        payload_from_env(&env),
                        &inner.lang,
                    ),
                );
                return Ok(());
            }
        }
    }

    fire_handlers(
        inner.clone(),
        "message.inbound",
        &env.account,
        &env.channel,
        env.conversation.kind.as_str(),
        &env.conversation.peer,
        &env.sender,
        &text,
        payload_from_env(&env),
    )
    .await;

    let direct = dm || mentioned || reply;
    enqueue(inner, env, direct).await
}

async fn enqueue(inner: Arc<Inner>, env: Envelope, direct: bool) -> Result<()> {
    let key = env.key();
    let mut convs = inner.convs.lock().await;
    let st = convs.entry(key.clone()).or_insert_with(|| ConvState {
        epoch: 0,
        running: false,
        pending: Vec::new(),
        steer: Vec::new(),
    });
    if st.running {
        st.steer.push(env);
        return Ok(());
    }
    let first_idle = !direct && st.pending.is_empty();
    st.pending.push(env);
    if direct {
        st.epoch += 1;
        let epoch = st.epoch;
        drop(convs);
        spawn_fire(inner, key, epoch, false);
    } else if first_idle {
        st.epoch += 1;
        let epoch = st.epoch;
        drop(convs);
        spawn_fire(inner, key, epoch, true);
    }
    Ok(())
}

fn spawn_fire(inner: Arc<Inner>, key: ConvKey, epoch: u64, idle: bool) {
    let wait = if idle { inner.idle } else { inner.debounce };
    tokio::spawn(async move {
        tokio::time::sleep(wait).await;
        if let Err(e) = fire(&inner, &key, epoch, idle).await {
            tracing::warn!("fire: {e}");
        }
    });
}


async fn fire(inner: &Inner, key: &ConvKey, epoch: u64, idle: bool) -> Result<()> {
    let (account, batch) = {
        let mut convs = inner.convs.lock().await;
        let Some(st) = convs.get_mut(key) else {
            return Ok(());
        };
        if st.epoch != epoch || st.running || st.pending.is_empty() {
            return Ok(());
        }
        st.running = true;
        let account = st.pending[0].account.clone();
        let batch = std::mem::take(&mut st.pending);
        (account, batch)
    };
    if let Err(e) = run_prompt(inner, key, &account, batch, idle).await {
        tracing::warn!("agent: {e}");
    }
    drain_steer(inner, key).await;
    Ok(())
}

async fn prompt_wait_run(inner: Arc<Inner>, env: Envelope) -> Result<String> {
    let key = env.key();
    let account = env.account.clone();
    loop {
        {
            let mut convs = inner.convs.lock().await;
            let st = convs.entry(key.clone()).or_insert_with(|| ConvState {
                epoch: 0,
                running: false,
                pending: Vec::new(),
                steer: Vec::new(),
            });
            if !st.running && st.pending.is_empty() {
                st.running = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let result = run_prompt(&inner, &key, &account, vec![env], false).await;
    drain_steer(&inner, &key).await;
    result
}

async fn run_prompt(
    inner: &Inner,
    key: &ConvKey,
    account: &str,
    batch: Vec<Envelope>,
    idle: bool,
) -> Result<String> {
    tracing::info!(conv = %key.as_list_key(), idle, n = batch.len(), "agent prompt");
    let pack = fetch_pack(inner, key, &batch).await.unwrap_or_default();
    let self_id = inner.self_ids.lock().await.get(account).cloned();
    let input = AgentInput {
        idle,
        dream: false,
        account: account.to_string(),
        self_id,
        key: key.clone(),
        messages: batch,
        pack,
    };
    inner.backend.prompt(key, input).await.map(|out| out.text)
}

async fn drain_steer(inner: &Inner, key: &ConvKey) {
    loop {
        let extra = {
            let mut convs = inner.convs.lock().await;
            let Some(st) = convs.get_mut(key) else {
                return;
            };
            let extra = std::mem::take(&mut st.steer);
            if extra.is_empty() {
                st.running = false;
                None
            } else {
                Some(extra)
            }
        };
        let Some(extra) = extra else {
            break;
        };
        if let Err(e) = flush_steer_batch(inner, key, extra).await {
            tracing::warn!("steer: {e}");
        }
    }
}

async fn flush_steer_batch(inner: &Inner, key: &ConvKey, batch: Vec<Envelope>) -> Result<()> {
    tracing::info!(conv = %key.as_list_key(), n = batch.len(), "agent steer");
    let text = batch
        .iter()
        .map(Envelope::prompt_line)
        .collect::<Vec<_>>()
        .join("\n");
    inner.backend.steer(key, &text).await
}

async fn fetch_pack(inner: &Inner, key: &ConvKey, batch: &[Envelope]) -> Result<crate::types::Pack> {
    match key.kind {
        ConversationKind::Group => {
            inner
                .memory
                .pack(&key.channel, key.kind, &key.peer, None)
                .await
        }
        ConversationKind::Dm => {
            let person = batch.first().map(|e| e.sender.id.as_str());
            inner
                .memory
                .pack(&key.channel, key.kind, &key.peer, person)
                .await
        }
    }
}


fn spawn_run(inner: Arc<Inner>, req: crate::daemon::RunRequest) {
    tokio::spawn(async move {
        let d = inner.daemon.lock().await.clone();
        if let Some(d) = d {
            if let Err(e) = d.run(req).await {
                tracing::warn!("daemon run: {e}");
            }
        }
    });
}

async fn fire_handlers(
    inner: Arc<Inner>,
    event: &str,
    account: &str,
    channel: &str,
    kind: &str,
    peer: &str,
    sender: &crate::types::Actor,
    text: &str,
    payload: serde_json::Value,
) {
    let names: Vec<String> = inner
        .registry
        .lock()
        .await
        .matching_handlers(event, account, kind, peer)
        .into_iter()
        .map(|h| h.name.clone())
        .collect();
    if names.is_empty() {
        return;
    }
    let conv = Conversation {
        kind: ConversationKind::parse(kind).unwrap_or(ConversationKind::Dm),
        peer: peer.into(),
    };
    let lang = inner.lang.clone();
    for name in names {
        spawn_run(
            inner.clone(),
            run_json(
                "handler",
                &name,
                account,
                channel,
                &conv,
                sender,
                text,
                Vec::new(),
                event,
                payload.clone(),
                &lang,
            ),
        );
    }
}

 
async fn busy(inner: &Inner) -> bool {
    if *inner.dreaming.lock().await {
        return true;
    }
    inner.convs.lock().await.values().any(|s| s.running)
}

async fn run_dream(inner: &Inner) {
    {
        let mut dreaming = inner.dreaming.lock().await;
        if *dreaming {
            return;
        }
        *dreaming = true;
    }
    tracing::info!("dream start");
    let key = dream_key();
    let pack = inner
        .memory
        .pack(&key.channel, key.kind, &key.peer, None)
        .await
        .unwrap_or_default();
    let input = AgentInput {
        idle: false,
        dream: true,
        account: "dream".into(),
        self_id: None,
        key: key.clone(),
        messages: Vec::new(),
        pack,
    };
    if let Err(e) = inner.backend.prompt(&key, input).await {
        tracing::warn!("dream: {e}");
    }
    *inner.dreaming.lock().await = false;
    inner.dream.lock().await.last = Instant::now();
}

fn next_cron_ts(expr: &str) -> Option<i64> {
    Schedule::from_str(expr)
        .ok()?
        .upcoming(Utc)
        .next()
        .map(|t| t.timestamp())
}

fn preview(s: &str) -> String {
    const N: usize = 80;
    let t = s.trim();
    let mut it = t.chars();
    let head: String = it.by_ref().take(N).collect();
    if it.next().is_none() {
        head
    } else {
        format!("{head}…")
    }
}


fn cmd(lang: &str, key: &str) -> &'static str {
    let lang = lang.to_ascii_lowercase();
    let zh = matches!(lang.as_str(), "zh" | "zh-cn" | "zh_cn" | "cn");
    match (zh, key) {
        (true, "session_reset") => "会话已重置",
        (_, "session_reset") => "session reset",
        (true, "help_title") => "命令：",
        (_, "help_title") => "Commands:",
        (true, "help_help") => "查看可用命令",
        (_, "help_help") => "list commands",
        (true, "help_new") => "重置会话（仅 owners）",
        (_, "help_new") => "reset session (owners)",
        _ => "",
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

fn format_help(lang: &str, commands: &[crate::daemon::CommandReg]) -> String {
    let mut s = format!("{}\n/help — {}\n/new — {}\n", cmd(lang, "help_title"), cmd(lang, "help_help"), cmd(lang, "help_new"));
    for c in commands {
        s.push('/');
        s.push_str(&c.name);
        if c.level == "owner" {
            s.push_str(" [owner]");
        }
        let intro = first_line(&c.docs);
        if !intro.is_empty() {
            s.push_str(" — ");
            s.push_str(intro);
        }
        s.push('\n');
    }
    s
}
