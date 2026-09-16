#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo build --release -p gladys-channel -p gladys-memory -p gladys-gateway -p gladys-scheduler

rm -rf dist/gladys
mkdir -p dist/gladys/bin dist/gladys/scripts dist/gladys/docker

cp target/release/gladys-channel target/release/gladys-memory \
  target/release/gladys-gateway target/release/gladys-scheduler dist/gladys/bin/
chmod +x dist/gladys/bin/*

cp scripts/init.sh scripts/start.sh scripts/stop.sh scripts/stop-host.sh \
  scripts/shell.sh scripts/acp.sh dist/gladys/scripts/
chmod +x dist/gladys/scripts/*

cp Dockerfile docker-compose.yml justfile dist/gladys/
cp docker/entrypoint.sh docker/scheduler.toml dist/gladys/docker/
cp channel.toml.example memory.toml.example gateway.toml.example scheduler.toml.example dist/gladys/
cp -r examples dist/gladys/

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
tar -C dist -czf "dist/gladys-${os}-${arch}.tar.gz" gladys
echo "wrote dist/gladys/ and dist/gladys-${os}-${arch}.tar.gz"
