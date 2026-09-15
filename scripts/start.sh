#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/init.sh --quiet
set -a
# shellcheck disable=SC1091
source workspace/.env
set +a
cargo build --release -p gladys-channel -p gladys-memory -p gladys-gateway -p gladys-scheduler
docker compose --env-file workspace/.env up -d --build
mkdir -p workspace/run

wait_port() {
  local port=$1 pidfile=$2
  for _ in $(seq 1 50); do
    if ! kill -0 "$(cat "$pidfile")" 2>/dev/null; then
      echo "process for :$port exited" >&2
      exit 1
    fi
    if (echo >/dev/tcp/127.0.0.1/"$port") 2>/dev/null; then
      return 0
    fi
    sleep 0.1
  done
  echo "timeout waiting for :$port" >&2
  exit 1
}

./target/release/gladys-channel --config workspace/channel.toml & echo $! > workspace/run/channel.pid
wait_port 3920 workspace/run/channel.pid
./target/release/gladys-memory --config workspace/memory.toml & echo $! > workspace/run/memory.pid
wait_port 3921 workspace/run/memory.pid
./target/release/gladys-gateway --config workspace/gateway.toml & echo $! > workspace/run/gateway.pid
echo "channel/memory/gateway up. Ctrl+C stops host servers (docker stays)."
trap './scripts/stop-host.sh' EXIT INT TERM
wait
