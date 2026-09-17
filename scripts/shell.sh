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

if [[ "$docker" -eq 1 ]]; then
  if [[ -x "$ROOT/bin/gladys-daemon" ]]; then
    BIN="$ROOT/bin"
  elif [[ -f "$ROOT/Cargo.toml" ]]; then
    CC=musl-gcc cargo build --release --target x86_64-unknown-linux-musl -p gladys-daemon
    BIN="$ROOT/target/x86_64-unknown-linux-musl/release"
  else
    echo "missing gladys-daemon binary" >&2
    exit 1
  fi
  mkdir -p workspace/run
  touch workspace/run/docker.mode
  export GLADYS_DAEMON_BIN="$BIN/gladys-daemon"
  ./scripts/compose-up.sh
  exec docker exec -it -w /workspace gladys-agent bash
fi

export HOME="$ROOT/workspace"
mkdir -p workspace/home
cd workspace/home
exec "${SHELL:-bash}"
