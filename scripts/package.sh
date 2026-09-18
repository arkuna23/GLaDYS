#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CC=musl-gcc cargo build --release --target x86_64-unknown-linux-musl \
  -p gladys-channel -p gladys-memory -p gladys-gateway -p gladys-daemon -p gladys-web

rm -rf dist/gladys
mkdir -p dist/gladys/bin dist/gladys/scripts dist/gladys/docker

cp target/x86_64-unknown-linux-musl/release/gladys-channel \
  target/x86_64-unknown-linux-musl/release/gladys-memory \
  target/x86_64-unknown-linux-musl/release/gladys-gateway \
  target/x86_64-unknown-linux-musl/release/gladys-daemon \
  target/x86_64-unknown-linux-musl/release/gladys-web dist/gladys/bin/
chmod +x dist/gladys/bin/*

cp scripts/init.sh scripts/start.sh scripts/stop.sh scripts/stop-host.sh \
  scripts/shell.sh scripts/acp.sh scripts/compose-up.sh dist/gladys/scripts/
chmod +x dist/gladys/scripts/*

cp Dockerfile docker-compose.yml justfile dist/gladys/
cp docker/entrypoint.sh docker/daemon.toml dist/gladys/docker/
cp channel.toml.example memory.toml.example gateway.toml.example daemon.toml.example web.toml.example dist/gladys/
cp -r examples dist/gladys/
cp -r skills dist/gladys/

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
tar -C dist -czf "dist/gladys-${os}-${arch}.tar.gz" gladys
echo "wrote dist/gladys/ and dist/gladys-${os}-${arch}.tar.gz"
