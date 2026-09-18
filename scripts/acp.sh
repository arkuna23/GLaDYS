#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -f "$ROOT/workspace/run/docker.mode" ]]; then
  exec docker exec -i -w /workspace -e PATH=/usr/local/bin:/usr/local/sbin:/usr/bin:/sbin:/bin gladys-agent \
    sh -c 'command -v pi >/dev/null 2>&1 || npm install -g @earendil-works/pi-coding-agent pi-acp pi-mcp-adapter; exec pi-acp'
fi
export HOME="$ROOT/workspace/home"
mkdir -p "$ROOT/workspace/workspace"
cd "$ROOT/workspace/workspace"
exec pi-acp "$@"
