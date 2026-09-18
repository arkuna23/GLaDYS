#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."
for f in workspace/run/channel.pid workspace/run/memory.pid workspace/run/gateway.pid workspace/run/web.pid workspace/run/daemon.pid; do
  if [[ -f "$f" ]]; then
    kill "$(cat "$f")" 2>/dev/null || true
    rm -f "$f"
  fi
done
