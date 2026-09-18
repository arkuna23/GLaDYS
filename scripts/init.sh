#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
quiet=0
[[ "${1:-}" == "--quiet" ]] && quiet=1
mkdir -p workspace/workspace workspace/home/.pi/agent/skills workspace/data workspace/run

tok() { python3 -c 'import secrets; print(secrets.token_hex(16))'; }

copy_example() {
  local src=$1 dest=$2
  if [[ -f "$dest" || ! -f "$src" ]]; then
    return
  fi
  cp "$src" "$dest"
  echo "wrote $dest"
}

if [[ ! -f workspace/.env ]]; then
  cat > workspace/.env <<EOF
GLADYS_CHANNEL_TOKEN=$(tok)
GLADYS_MEMORY_TOKEN=$(tok)
GLADYS_GATEWAY_TOKEN=$(tok)
GLADYS_DAEMON_TOKEN=$(tok)
GLADYS_WEB_TOKEN=$(tok)
NAPCAT_TOKEN=
EOF
  echo "wrote workspace/.env (tokens)"
fi
if ! grep -q '^GLADYS_WEB_TOKEN=' workspace/.env; then
  echo "GLADYS_WEB_TOKEN=$(tok)" >> workspace/.env
  echo "appended GLADYS_WEB_TOKEN to workspace/.env"
fi

copy_example channel.toml.example workspace/channel.toml
copy_example memory.toml.example workspace/memory.toml
copy_example gateway.toml.example workspace/gateway.toml
copy_example daemon.toml.example workspace/daemon.toml
copy_example web.toml.example workspace/web.toml
copy_example examples/pi/models.json workspace/home/.pi/agent/models.json
copy_example examples/pi/settings.json workspace/home/.pi/agent/settings.json
copy_example examples/pi/mcp.json workspace/home/.pi/agent/mcp.json
rm -rf workspace/home/.pi/agent/skills/gladys-lua
cp -a skills/gladys-lua workspace/home/.pi/agent/skills/gladys-lua
chmod +x workspace/home/.pi/agent/skills/gladys-lua/scripts/*.py

if [[ "$quiet" -eq 0 ]]; then
  echo
  echo "Next:"
  echo "  1. Edit workspace/channel.toml and workspace/.env"
  echo "  2. Edit workspace/gateway.toml — owners / allowed groups"
  echo "  3. Edit workspace/home/.pi/agent/models.json — provider and model"
  echo "  4. ./scripts/start.sh            # or --docker for agent+daemon in Docker"
fi
