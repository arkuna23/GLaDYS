use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::agent::AgentBackend;
use crate::error::Result;
use crate::io::{ChannelIo, MemoryIo};
use crate::policy::{is_new_command, Policy};
use crate::store::Store;
use crate::types::{AgentInput, ConversationKind, ConvKey, Envelope, Part};

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
    self_ids: Mutex<HashMap<String, String>>,
    convs: Mutex<HashMap<ConvKey, ConvState>>,
}

struct ConvState {
    epoch: u64,
    running: bool,
    pending: Vec<Envelope>,
    steer: Vec<Envelope>,
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
                self_ids: Mutex::new(HashMap::new()),
                convs: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub async fn set_self_id(&self, account: String, self_id: String) {
        self.inner.self_ids.lock().await.insert(account, self_id);
    }

    pub async fn handle(&self, env: Envelope) -> Result<()> {
        handle(self.inner.clone(), env).await
    }

    pub async fn handle_scheduler(&self, mut env: Envelope) -> Result<String> {
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
            "scheduler trigger"
        );
        enqueue(self.inner.clone(), env, true).await?;
        Ok(id)
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
                    text: "session reset".into(),
                }],
            )
            .await?;
        inner.convs.lock().await.remove(&key);
        return Ok(());
    }

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
        st.epoch += 1;
        let epoch = st.epoch;
        drop(convs);
        spawn_steer(inner, key, epoch);
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

fn spawn_steer(inner: Arc<Inner>, key: ConvKey, epoch: u64) {
    let wait = inner.debounce;
    tokio::spawn(async move {
        tokio::time::sleep(wait).await;
        if let Err(e) = flush_steer(&inner, &key, epoch).await {
            tracing::warn!("steer: {e}");
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

    tracing::info!(conv = %key.as_list_key(), idle, n = batch.len(), "agent prompt");
    let pack = fetch_pack(inner, key, &batch).await.unwrap_or_default();
    let input = AgentInput {
        idle,
        account: account.clone(),
        key: key.clone(),
        messages: batch,
        pack,
    };
    let out = inner.backend.prompt(key, input).await;
    {
        let mut convs = inner.convs.lock().await;
        if let Some(st) = convs.get_mut(key) {
            st.running = false;
        }
    }
    if let Err(e) = out {
        tracing::warn!("agent: {e}");
    }
    Ok(())
}

async fn flush_steer(inner: &Inner, key: &ConvKey, epoch: u64) -> Result<()> {
    let batch = {
        let mut convs = inner.convs.lock().await;
        let Some(st) = convs.get_mut(key) else {
            return Ok(());
        };
        if st.epoch != epoch {
            return Ok(());
        }
        std::mem::take(&mut st.steer)
    };
    if batch.is_empty() {
        return Ok(());
    }
    tracing::info!(conv = %key.as_list_key(), n = batch.len(), "agent steer");
    let text = batch
        .iter()
        .map(Envelope::flatten_text)
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
