#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

docker=0
for a in "$@"; do
  case "$a" in
    --docker) docker=1 ;;
  esac
done
[[ -f workspace/run/docker.mode ]] && docker=1

./scripts/stop-host.sh
if [[ "$docker" -eq 1 ]]; then
  docker compose --env-file workspace/.env down || true
fi
rm -f workspace/run/docker.mode
