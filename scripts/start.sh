#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

docker=0
for a in "$@"; do
  case "$a" in
    --docker) docker=1 ;;
    *)
      echo "unknown arg: $a" >&2
      exit 1
      ;;
  esac
done

./scripts/init.sh --quiet
set -a
# shellcheck disable=SC1091
source workspace/.env
set +a

if [[ -x "$ROOT/bin/gladys-channel" ]]; then
  BIN="$ROOT/bin"
elif [[ -f "$ROOT/Cargo.toml" ]]; then
  cargo build --release -p gladys-channel -p gladys-memory -p gladys-gateway -p gladys-scheduler
  BIN="$ROOT/target/release"
else
  echo "missing binaries (bin/) and no Cargo.toml to build" >&2
  exit 1
fi

mkdir -p workspace/run
export GLADYS_AGENT_COMMAND="$ROOT/scripts/acp.sh"

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

if [[ "$docker" -eq 1 ]]; then
  touch workspace/run/docker.mode
  export GLADYS_SCHEDULER_BIN="$BIN/gladys-scheduler"
  export GLADYS_ACP_CWD=/workspace
  docker compose --env-file workspace/.env up -d --build
else
  rm -f workspace/run/docker.mode
  if ! command -v pi-acp >/dev/null 2>&1; then
    echo "pi-acp not on PATH; install it or use --docker" >&2
    exit 1
  fi
  export GLADYS_ACP_CWD="$ROOT/workspace/home"
fi
trap './scripts/stop-host.sh' EXIT INT TERM

"$BIN/gladys-channel" --config workspace/channel.toml & echo $! > workspace/run/channel.pid
wait_port 3920 workspace/run/channel.pid
"$BIN/gladys-memory" --config workspace/memory.toml & echo $! > workspace/run/memory.pid
wait_port 3921 workspace/run/memory.pid
"$BIN/gladys-gateway" --config workspace/gateway.toml & echo $! > workspace/run/gateway.pid
wait_port 3922 workspace/run/gateway.pid

if [[ "$docker" -eq 0 ]]; then
  "$BIN/gladys-scheduler" --config workspace/scheduler.toml & echo $! > workspace/run/scheduler.pid
  wait_port 3923 workspace/run/scheduler.pid
  echo "channel/memory/gateway/scheduler up. Ctrl+C stops host."
else
  echo "channel/memory/gateway up. agent+scheduler in docker. Ctrl+C stops host (docker stays)."
fi

wait
