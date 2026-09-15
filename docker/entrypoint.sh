#!/bin/sh
set -e
export HOME="${HOME:-/root}"
export PATH="/usr/local/bin:${PATH}"
gladys-scheduler --config /etc/gladys/scheduler.toml &
exec sleep infinity
