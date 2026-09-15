#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/stop-host.sh
docker compose --env-file workspace/.env down
