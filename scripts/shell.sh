#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/init.sh --quiet
set -a
# shellcheck disable=SC1091
source workspace/.env
set +a
cargo build --release -p gladys-scheduler
docker compose --env-file workspace/.env up -d
exec docker exec -it -w /workspace gladys-agent bash
