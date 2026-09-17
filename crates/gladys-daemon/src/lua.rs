use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mlua::{HookTriggers, Lua, LuaOptions, LuaSerdeExt, StdLib, Value as LuaValue, VmState};
use serde_json::{json, Value};
use tokio::sync::oneshot;

use crate::channel_mcp::{RecTools, SharedTools};
use crate::client::GatewayClient;
use crate::error::{DaemonError, Result};
use crate::runner::{fill_channel_args, fill_job_args, fill_memory_args, Actor, Conversation, RunRequest};

struct AgentSlot {
    rx: Option<oneshot::Receiver<std::result::Result<String, String>>>,
    cache: Option<String>,
}



fn libs() -> StdLib {
    StdLib::COROUTINE
        | StdLib::TABLE
        | StdLib::STRING
        | StdLib::UTF8
        | StdLib::MATH
        | StdLib::IO
        | StdLib::OS
}
pub async fn run(
    script: &str,
    req: &RunRequest,
    channel: SharedTools,
    memory: SharedTools,
    gateway: GatewayClient,
    timeout: Duration,
) -> Result<()> {
    if script.trim().is_empty() {
        return Err(DaemonError::Invalid("empty script".into()));
    }
    let lua = Lua::new_with(libs(), LuaOptions::default())
        .map_err(|e| DaemonError::Invalid(e.to_string()))?;
    let _ = lua.set_memory_limit(8 * 1024 * 1024);
    bind(&lua, req, channel, memory, Some(gateway)).map_err(|e| DaemonError::Invalid(e.to_string()))?;
    let start = Instant::now();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(50_000),
        {
            let ticks = std::sync::atomic::AtomicU32::new(0);
            move |_lua, _debug| {
                let n = ticks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if start.elapsed() > timeout || n > 400 {
                    Err(mlua::Error::runtime("timeout"))
                } else {
                    Ok(VmState::Continue)
                }
            }
        }
    )
    .map_err(|e| DaemonError::Invalid(e.to_string()))?;
    match tokio::time::timeout(timeout, lua.load(script).exec_async()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(DaemonError::Invalid(e.to_string())),
        Err(_) => Err(DaemonError::Invalid("timeout".into())),
    }
}

pub async fn check(script: &str, timeout: Duration) -> Result<()> {
    let req = dummy_req();
    let channel: SharedTools = Arc::new(RecTools::default());
    let memory: SharedTools = Arc::new(RecTools::default());
    if script.trim().is_empty() {
        return Err(DaemonError::Invalid("empty script".into()));
    }
    let lua = Lua::new_with(libs(), LuaOptions::default())
        .map_err(|e| DaemonError::Invalid(e.to_string()))?;
    let _ = lua.set_memory_limit(8 * 1024 * 1024);
    bind(&lua, &req, channel, memory, None).map_err(|e| DaemonError::Invalid(e.to_string()))?;
    let start = Instant::now();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(50_000),
        {
            let ticks = std::sync::atomic::AtomicU32::new(0);
            move |_lua, _debug| {
                let n = ticks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if start.elapsed() > timeout || n > 400 {
                    Err(mlua::Error::runtime("timeout"))
                } else {
                    Ok(VmState::Continue)
                }
            }
        },
    )
    .map_err(|e| DaemonError::Invalid(e.to_string()))?;
    match tokio::time::timeout(timeout, lua.load(script).exec_async()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(DaemonError::Invalid(e.to_string())),
        Err(_) => Err(DaemonError::Invalid("timeout".into())),
    }
}

fn dummy_req() -> RunRequest {
    RunRequest {
        kind: "command".into(),
        name: "check".into(),
        account: "check".into(),
        channel: "onebot".into(),
        conversation: Conversation {
            kind: "group".into(),
            peer: "0".into(),
        },
        sender: Actor::default(),
        text: String::new(),
        argv: vec![],
        event: String::new(),
        payload: Value::Null,
        lang: "en".into(),
    }
}

fn bind(
    lua: &Lua,
    req: &RunRequest,
    channel: SharedTools,
    memory: SharedTools,
    gateway: Option<GatewayClient>,
) -> mlua::Result<()> {
    let globals = lua.globals();
    for name in ["package", "require"] {
        globals.set(name, LuaValue::Nil)?;
    }
    globals.set(
        "print",
        lua.create_function(|_, xs: mlua::MultiValue| {
            let s = xs
                .into_iter()
                .map(|v| match v {
                    LuaValue::String(s) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
                    other => format!("{other:?}"),
                })
                .collect::<Vec<_>>()
                .join("\t");
            tracing::info!("lua: {s}");
            Ok(())
        })?,
    )?;

    let ctx = json!({
        "account": req.account,
        "channel": req.channel,
        "kind": req.conversation.kind,
        "peer": req.conversation.peer,
        "sender": {"id": req.sender.id, "name": req.sender.name},
        "text": req.text,
        "argv": req.argv,
        "event": req.event,
        "payload": req.payload,
        "lang": req.lang,
        "name": req.name,
    });
    globals.set("ctx", lua.to_value(&ctx)?)?;
    globals.set(
        "channel",
        mcp_table(lua, req, channel, "channel", fill_channel_args)?,
    )?;
    globals.set(
        "memory",
        mcp_table(lua, req, memory, "memory", fill_memory_args)?,
    )?;
    match gateway {
        Some(gw) => {
            globals.set("job", job_table(lua, req, gw.clone())?)?;
            globals.set("agent", agent_table(lua, req, gw)?)?;
        }
        None => {
            globals.set("job", dry_job_table(lua)?)?;
            globals.set("agent", dry_agent_table(lua)?)?;
        }
    }
    Ok(())
}

fn lua_json(lua: &Lua, v: LuaValue) -> mlua::Result<Value> {
    match v {
        LuaValue::Nil => Ok(json!({})),
        other => lua.from_value(other),
    }
}

fn lua_err(e: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(e.to_string())
}

fn mcp_table(
    lua: &Lua,
    req: &RunRequest,
    tools: SharedTools,
    tool: &'static str,
    fill: fn(&mut Value, &RunRequest),
) -> mlua::Result<mlua::Table> {
    let table = lua.create_table()?;
    let mt = lua.create_table()?;
    let req = req.clone();
    mt.set(
        "__index",
        lua.create_function(move |lua, (_t, key): (mlua::Table, String)| {
            let tools = tools.clone();
            let req = req.clone();
            lua.create_async_function(move |lua, args: LuaValue| {
                let tools = tools.clone();
                let req = req.clone();
                let key = key.clone();
                async move {
                    let mut args = lua_json(&lua, args)?;
                    fill(&mut args, &req);
                    let name = if tool == "channel" && key == "call" {
                        "channel_call"
                    } else {
                        if let Some(obj) = args.as_object_mut() {
                            obj.insert("op".into(), json!(key));
                        }
                        tool
                    };
                    let v = tools.call(name, args).await.map_err(lua_err)?;
                    lua.to_value(&v)
                }
            })
        })?,
    )?;
    table.set_metatable(Some(mt))?;
    Ok(table)
}

fn job_table(lua: &Lua, req: &RunRequest, gateway: GatewayClient) -> mlua::Result<mlua::Table> {
    let table = lua.create_table()?;
    let req = req.clone();

    let make = |kind: &'static str| {
        let gw = gateway.clone();
        let req = req.clone();
        lua.create_async_function(move |lua, args: LuaValue| {
            let gw = gw.clone();
            let req = req.clone();
            async move {
                let mut args = lua_json(&lua, args)?;
                fill_job_args(&mut args, &req);
                if let Some(obj) = args.as_object_mut() {
                    obj.insert("kind".into(), json!(kind));
                }
                let v = gw.create(args).await.map_err(lua_err)?;
                lua.to_value(&v)
            }
        })
    };

    table.set("cron", make("cron")?)?;
    table.set("delay", make("delay")?)?;

    let gw = gateway.clone();
    table.set(
        "list",
        lua.create_async_function(move |lua, ()| {
            let gw = gw.clone();
            async move {
                let v = gw.list().await.map_err(lua_err)?;
                lua.to_value(&v)
            }
        })?,
    )?;

    let gw = gateway.clone();
    table.set(
        "cancel",
        lua.create_async_function(move |lua, id: String| {
            let gw = gw.clone();
            async move {
                let v = gw.cancel(&id).await.map_err(lua_err)?;
                lua.to_value(&v)
            }
        })?,
    )?;
    Ok(table)
}

fn agent_text(lua: &Lua, v: LuaValue) -> mlua::Result<String> {
    match v {
        LuaValue::String(s) => Ok(s.to_str()?.to_string()),
        other => {
            let j: Value = lua.from_value(other)?;
            if let Some(s) = j.as_str() {
                return Ok(s.to_string());
            }
            j.get("text")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .ok_or_else(|| mlua::Error::runtime("text required"))
        }
    }
}

fn agent_table(lua: &Lua, req: &RunRequest, gateway: GatewayClient) -> mlua::Result<mlua::Table> {
    let table = lua.create_table()?;
    let slot = Arc::new(Mutex::new(AgentSlot {
        rx: None,
        cache: None,
    }));
    let req = req.clone();

    let slot_p = slot.clone();
    let gw = gateway.clone();
    let req_p = req.clone();
    table.set(
        "prompt",
        lua.create_function(move |lua, (slf, text): (mlua::Table, LuaValue)| {
            let text = agent_text(lua, text)?;
            let mut g = slot_p.lock().map_err(|e| lua_err(e))?;
            if g.rx.is_some() {
                return Err(mlua::Error::runtime("prompt already running"));
            }
            let (tx, rx) = oneshot::channel();
            g.rx = Some(rx);
            g.cache = None;
            drop(g);
            let gw = gw.clone();
            let body = json!({
                "account": req_p.account,
                "channel": req_p.channel,
                "conversation": {"kind": req_p.conversation.kind, "peer": req_p.conversation.peer},
                "text": text,
            });
            tokio::spawn(async move {
                let r = match gw.prompt(body).await {
                    Ok(v) => Ok(v
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()),
                    Err(e) => Err(e.to_string()),
                };
                let _ = tx.send(r);
            });
            Ok(slf)
        })?,
    )?;

    let slot_w = slot.clone();
    table.set(
        "wait",
        lua.create_async_function(move |_, _slf: mlua::Table| {
            let slot = slot_w.clone();
            async move {
                let cache = slot.lock().map_err(lua_err)?.cache.clone();
                if let Some(c) = cache {
                    return Ok(c);
                }
                let rx = slot
                    .lock()
                    .map_err(lua_err)?
                    .rx
                    .take()
                    .ok_or_else(|| mlua::Error::runtime("no prompt"))?;
                let text = rx
                    .await
                    .map_err(|_| mlua::Error::runtime("prompt closed"))?
                    .map_err(lua_err)?;
                slot.lock().map_err(lua_err)?.cache = Some(text.clone());
                Ok(text)
            }
        })?,
    )?;

    let mt = lua.create_table()?;
    mt.set(
        "__call",
        lua.create_function(|_, (slf, text): (mlua::Table, LuaValue)| {
            let prompt: mlua::Function = slf.get("prompt")?;
            let _: mlua::Table = prompt.call((slf.clone(), text))?;
            Ok(slf)
        })?,
    )?;
    table.set_metatable(Some(mt))?;
    Ok(table)
}


fn dry_job_table(lua: &Lua) -> mlua::Result<mlua::Table> {
    let table = lua.create_table()?;
    let stub = lua.create_async_function(|lua, _: LuaValue| async move {
        lua.to_value(&json!({"ok": true, "id": "check"}))
    })?;
    table.set("cron", stub.clone())?;
    table.set("delay", stub.clone())?;
    table.set("list", stub.clone())?;
    table.set("cancel", stub)?;
    Ok(table)
}

fn dry_agent_table(lua: &Lua) -> mlua::Result<mlua::Table> {
    let table = lua.create_table()?;
    let slot = Arc::new(Mutex::new(AgentSlot {
        rx: None,
        cache: None,
    }));
    let slot_p = slot.clone();
    table.set(
        "prompt",
        lua.create_function(move |lua, (slf, text): (mlua::Table, LuaValue)| {
            let _ = agent_text(lua, text)?;
            let mut g = slot_p.lock().map_err(lua_err)?;
            if g.rx.is_some() || g.cache.is_some() {
                return Err(mlua::Error::runtime("prompt already running"));
            }
            g.cache = Some(String::new());
            Ok(slf)
        })?,
    )?;
    let slot_w = slot.clone();
    table.set(
        "wait",
        lua.create_async_function(move |_, _slf: mlua::Table| {
            let slot = slot_w.clone();
            async move {
                slot.lock()
                    .map_err(lua_err)?
                    .cache
                    .clone()
                    .ok_or_else(|| mlua::Error::runtime("no prompt"))
            }
        })?,
    )?;
    let mt = lua.create_table()?;
    mt.set(
        "__call",
        lua.create_function(|_, (slf, text): (mlua::Table, LuaValue)| {
            let prompt: mlua::Function = slf.get("prompt")?;
            let _: mlua::Table = prompt.call((slf.clone(), text))?;
            Ok(slf)
        })?,
    )?;
    table.set_metatable(Some(mt))?;
    Ok(table)
}
