#!/bin/sh
set -e
export HOME="${HOME:-/root}"
export PATH="/usr/local/bin:${PATH}"
if ! command -v pi >/dev/null 2>&1; then
  npm install -g @earendil-works/pi-coding-agent pi-acp pi-mcp-adapter
fi
gladys-daemon --config /etc/gladys/daemon.toml &
exec sleep infinity
