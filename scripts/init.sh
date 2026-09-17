#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
quiet=0
[[ "${1:-}" == "--quiet" ]] && quiet=1
mkdir -p workspace/home workspace/.pi/agent/skills workspace/data workspace/run

tok() { python3 -c 'import secrets; print(secrets.token_hex(16))'; }

copy_example() {
  local src=$1 dest=$2
  if [[ ! -f "$dest" ]]; then
    cp "$src" "$dest"
    echo "wrote $dest"
  fi
}

if [[ ! -f workspace/.env ]]; then
  cat > workspace/.env <<EOF
GLADYS_CHANNEL_TOKEN=$(tok)
GLADYS_MEMORY_TOKEN=$(tok)
GLADYS_GATEWAY_TOKEN=$(tok)
GLADYS_DAEMON_TOKEN=$(tok)
NAPCAT_TOKEN=
EOF
  echo "wrote workspace/.env (tokens)"
fi

copy_example channel.toml.example workspace/channel.toml
copy_example memory.toml.example workspace/memory.toml
copy_example gateway.toml.example workspace/gateway.toml
copy_example daemon.toml.example workspace/daemon.toml
copy_example examples/pi/models.json workspace/.pi/agent/models.json
copy_example examples/pi/settings.json workspace/.pi/agent/settings.json
copy_example examples/pi/mcp.json workspace/.pi/agent/mcp.json
rm -rf workspace/.pi/agent/skills/gladys-lua
cp -a skills/gladys-lua workspace/.pi/agent/skills/gladys-lua
chmod +x workspace/.pi/agent/skills/gladys-lua/scripts/*.py

if [[ "$quiet" -eq 0 ]]; then
  echo
  echo "Next:"
  echo "  1. Edit workspace/channel.toml and workspace/.env"
  echo "  2. Edit workspace/gateway.toml — owners / allowed groups"
  echo "  3. Edit workspace/.pi/agent/models.json — provider and model"
  echo "  4. ./scripts/start.sh            # or --docker for agent+daemon in Docker"
fi
