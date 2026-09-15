#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
quiet=0
[[ "${1:-}" == "--quiet" ]] && quiet=1
mkdir -p workspace/home workspace/.pi/agent workspace/data workspace/run
tok() { python3 -c 'import secrets; print(secrets.token_hex(16))'; }
if [[ ! -f workspace/.env ]]; then
  cat > workspace/.env <<EOF
GLADYS_CHANNEL_TOKEN=$(tok)
GLADYS_MEMORY_TOKEN=$(tok)
GLADYS_GATEWAY_TOKEN=$(tok)
GLADYS_SCHEDULER_TOKEN=$(tok)
NAPCAT_TOKEN=
EOF
  echo "wrote workspace/.env (tokens)"
fi
if [[ ! -f workspace/channel.toml ]]; then
  cat > workspace/channel.toml <<'EOF'
bind = "127.0.0.1:3920"
data_dir = "workspace/data/channel"
token_env = "GLADYS_CHANNEL_TOKEN"
debug = false

[[accounts]]
id = "qq-main"
channel = "onebot"
profile = "napcat"
mode = "forward_ws"
ws_url = "ws://127.0.0.1:3001"
access_token_env = "NAPCAT_TOKEN"
download_media = true
EOF
  echo "wrote workspace/channel.toml"
fi
if [[ ! -f workspace/memory.toml ]]; then
  cat > workspace/memory.toml <<'EOF'
bind = "127.0.0.1:3921"
data_dir = "workspace/data/memory"
token_env = "GLADYS_MEMORY_TOKEN"
pack_limit = 20
pack_max_chars = 4000
EOF
  echo "wrote workspace/memory.toml"
fi
if [[ ! -f workspace/gateway.toml ]]; then
  cat > workspace/gateway.toml <<'EOF'
bind = "127.0.0.1:3922"
data_dir = "workspace/data/gateway"
token_env = "GLADYS_GATEWAY_TOKEN"
channel_ws = "ws://127.0.0.1:3920/v1/gateway"
channel_token_env = "GLADYS_CHANNEL_TOKEN"
memory_url = "http://127.0.0.1:3921"
memory_token_env = "GLADYS_MEMORY_TOKEN"
idle_group_secs = 30
debounce_ms = 1000
owners = ["onebot:123456"]

[agent]
kind = "acp"
command = "docker"
args = ["exec", "-i", "-w", "/workspace", "gladys-agent", "pi-acp"]
cwd = "/workspace"

[policy]
group_mode = "whitelist"
dm_mode = "blacklist"
groups = ["onebot:group:477517182"]
dms = []

[[mcp]]
name = "channel"
url = "http://127.0.0.1:3920/mcp"
token_env = "GLADYS_CHANNEL_TOKEN"

[[mcp]]
name = "memory"
url = "http://127.0.0.1:3921/mcp"
token_env = "GLADYS_MEMORY_TOKEN"

[[mcp]]
name = "scheduler"
url = "http://127.0.0.1:3923/mcp"
token_env = "GLADYS_SCHEDULER_TOKEN"
EOF
  echo "wrote workspace/gateway.toml"
fi
if [[ ! -f workspace/scheduler.toml ]]; then
  cat > workspace/scheduler.toml <<'EOF'
bind = "0.0.0.0:3923"
token_env = "GLADYS_SCHEDULER_TOKEN"
gateway_url = "http://127.0.0.1:3922"
gateway_token_env = "GLADYS_GATEWAY_TOKEN"
EOF
  echo "wrote workspace/scheduler.toml"
fi
if [[ ! -f workspace/.pi/agent/models.json ]]; then
  cat > workspace/.pi/agent/models.json <<'EOF'
{
  "providers": {
    "sub2api": {
      "baseUrl": "http://192.168.2.100:8316",
      "api": "openai-responses",
      "apiKey": "$OPENAI_API_KEY",
      "models": [
        {
          "id": "deepseek-flash",
          "name": "DeepSeek V4.1 Flash",
          "reasoning": true,
          "thinkingLevelMap": {
            "low": "low",
            "high": "high",
            "max": "max"
          },
          "input": ["text", "image"],
          "cost": {
            "input": 0.15,
            "output": 0.6,
            "cacheRead": 0.003,
            "cacheWrite": 0
          },
          "contextWindow": 1000000,
          "maxTokens": 384000,
          "compat": {
            "requiresReasoningContentOnAssistantMessages": true
          }
        }
      ]
    }
  }
}
EOF
  echo "wrote workspace/.pi/agent/models.json"
fi
if [[ ! -f workspace/.pi/agent/settings.json ]]; then
  cat > workspace/.pi/agent/settings.json <<'EOF'
{
  "defaultProvider": "sub2api",
  "defaultModel": "deepseek-flash",
  "defaultThinkingLevel": "high",
  "defaultProjectTrust": "always",
  "packages": ["/usr/local/lib/node_modules/pi-mcp-adapter"]
}
EOF
  echo "wrote workspace/.pi/agent/settings.json"
fi
if [[ ! -f workspace/.pi/agent/mcp.json ]]; then
  cat > workspace/.pi/agent/mcp.json <<'EOF'
{
  "settings": { "directTools": true },
  "mcpServers": {
    "channel": {
      "type": "http",
      "url": "http://127.0.0.1:3920/mcp",
      "headers": { "Authorization": "Bearer ${GLADYS_CHANNEL_TOKEN}" },
      "directTools": true,
      "lifecycle": "eager"
    },
    "memory": {
      "type": "http",
      "url": "http://127.0.0.1:3921/mcp",
      "headers": { "Authorization": "Bearer ${GLADYS_MEMORY_TOKEN}" },
      "directTools": true,
      "lifecycle": "eager"
    },
    "scheduler": {
      "type": "http",
      "url": "http://127.0.0.1:3923/mcp",
      "headers": { "Authorization": "Bearer ${GLADYS_SCHEDULER_TOKEN}" },
      "directTools": true,
      "lifecycle": "eager"
    }
  }
}
EOF
  echo "wrote workspace/.pi/agent/mcp.json"
fi
if [[ "$quiet" -eq 0 ]]; then
  echo
  echo "Next:"
  echo "  1. Edit workspace/channel.toml  — NapCat/OneBot (ws_url, NAPCAT_TOKEN in workspace/.env)"
  echo "  2. Edit workspace/gateway.toml  — owners / allowed groups"
  echo "  3. just setup  — enter the agent container and configure pi yourself"
  echo "  4. just start"
fi
