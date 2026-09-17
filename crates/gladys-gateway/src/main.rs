use std::sync::Arc;

use anyhow::{Context, Result};
use gladys_gateway::acp::AcpBackend;
use gladys_gateway::channel::ChannelWs;
use gladys_gateway::config::{config_path_from_args, Config};
use gladys_gateway::daemon::HttpDaemon;
use gladys_gateway::dispatch::Dispatch;
use gladys_gateway::http::{self, AppState};
use gladys_gateway::io::ChannelSlot;
use gladys_gateway::memory::MemoryHttp;
use gladys_gateway::policy::Policy;
use gladys_gateway::store::Store;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gladys_gateway=info".into()),
        )
        .init();

    let path = config_path_from_args();
    let cfg = Config::load_path(&path).with_context(|| format!("load {}", path.display()))?;
    if cfg.agent.kind != "acp" {
        anyhow::bail!("only agent.kind = acp is supported");
    }
    let (agent_cmd, agent_args) = match std::env::var("GLADYS_AGENT_COMMAND") {
        Ok(cmd) if !cmd.is_empty() => (cmd, Vec::new()),
        _ => (cfg.agent.command.clone(), cfg.agent.args.clone()),
    };
    let agent_cwd = std::env::var("GLADYS_ACP_CWD")
        .ok()
        .filter(|s| !s.is_empty())
        .or(cfg.agent.cwd.clone());
    std::fs::create_dir_all(&cfg.data_dir)?;
    let store = Store::open(cfg.data_dir.join("gateway.db"))?;
    let slot = Arc::new(ChannelSlot::default());
    let dispatch = Dispatch::new(
        Policy {
            group_mode: cfg.group_mode(),
            dm_mode: cfg.dm_mode(),
            groups: cfg.policy.groups.clone(),
            dms: cfg.policy.dms.clone(),
            owners: cfg.owners.clone(),
        },
        cfg.debounce(),
        cfg.idle(),
        Arc::new(AcpBackend::new(
            agent_cmd,
            agent_args,
            agent_cwd,
            cfg.mcp.clone(),
            store.clone(),
        )),
        slot.clone(),
        Arc::new(MemoryHttp::new(cfg.memory_url.clone(), cfg.memory_token()?)),
        store.clone(),
        cfg.lang.clone(),
    );
    dispatch
        .set_daemon(std::sync::Arc::new(HttpDaemon::new(
            cfg.daemon_url.clone(),
            cfg.daemon_token()?,
        )))
        .await;
    if cfg.dream.mode != "off" {
        dispatch.configure_dream(cfg.dream.clone()).await;
        dispatch.spawn_dream_loop();
    }
    let jobs = gladys_gateway::Jobs::new(store, dispatch.clone());
    jobs.start();
    let ws = ChannelWs::connect(&cfg.channel_ws, &cfg.channel_token()?, dispatch.clone()).await?;
    slot.set(ws).await;
    http::serve(
        &cfg.bind,
        AppState {
            token: cfg.token()?,
            dispatch,
            jobs,
        },
    )
    .await?;
    std::process::exit(0);
}
