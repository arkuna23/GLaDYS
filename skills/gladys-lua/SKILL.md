---
name: gladys-lua
description: Write, dry-run, and upload GLaDYS daemon Lua chat commands and event handlers. Use when creating or editing /commands, handlers, daemon Lua, gladys scripts, or uploading ping.lua-style bots.
---

# GLaDYS Lua scripts

Optional. Save Lua by HTTP upload (not MCP JSON). Helpers are Python 3 (stdlib only).

## Workflow

1. Write a `.lua` file.
2. Check (daemon up):

```bash
export GLADYS_DAEMON_URL="${GLADYS_DAEMON_URL:-http://127.0.0.1:3923}"
export GLADYS_DAEMON_TOKEN
python3 "${SKILL_DIR}/scripts/gladys_lua.py" check ./ping.lua
```

3. Upload:

```bash
python3 "${SKILL_DIR}/scripts/gladys_lua.py" upload command ping ./ping.lua --level user --docs "pong"
python3 "${SKILL_DIR}/scripts/gladys_lua.py" upload handler onmsg ./onmsg.lua --event message.inbound
```

`SKILL_DIR` is this skill directory. Token: `GLADYS_DAEMON_TOKEN` from `workspace/.env`.

Check dry-runs mock `channel` / `memory` / `agent` / `job`. Upload is `PUT /v1/scripts/{name}` with `X-Gladys-Type`.

## Lua API

Lua 5.4. No `require` / `package` (no importing other Lua modules). Other libraries (`os`, `io`, `string`, `math`, …) are available.

`ctx`: `account`, `channel`, `kind`, `peer`, `sender{id,name}`, `text`, `argv`, `event`, `payload`, `lang`, `name`.

```lua
channel.send{ parts = { { type = "text", text = "pong" } } }
agent:prompt("summarize")
local text = agent:wait()
memory.write{ layer = "conversation", text = "note" }
job.delay{ after_secs = 60, text = "later" }
```

Missing `account`/`conversation` filled from `ctx`. Failures are Lua errors. `print` logs only. ACP text is not sent unless the script `channel.send`s it. MCP wire: `channel`/`memory` tools take `op` (`channel.send` → `{op:"send", ...}`); native is `channel.call`.

Chat: group needs @ + `/name`; DM starts with `/name`. level `user` or `owner`.
