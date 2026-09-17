#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

need_build() {
  if ! docker image inspect gladys-agent >/dev/null 2>&1; then
    return 0
  fi
  local img
  img=$(date -u -d "$(docker image inspect -f '{{.Created}}' gladys-agent)" +%s)
  local f
  for f in Dockerfile docker/entrypoint.sh docker/daemon.toml; do
    [[ -e $f ]] || continue
    if [[ $(stat -c %Y "$f") -gt $img ]]; then
      return 0
    fi
  done
  return 1
}

if need_build; then
  docker compose --env-file workspace/.env up -d --build
else
  docker compose --env-file workspace/.env up -d
fi
