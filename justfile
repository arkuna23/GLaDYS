set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @just --list

# Generate workspace configs (skip files that already exist).
init:
    ./scripts/init.sh

# Release binaries + scripts + examples into dist/gladys/ and a tar.gz.
package:
    ./scripts/package.sh

# Shell into workspace (pass --docker for the agent container).
shell *args:
    ./scripts/shell.sh {{args}}

# Host Channel/Memory/Gateway/Web. Use `just start -- --docker` for containers.
start *args:
    ./scripts/start.sh {{args}}


# Agent + daemon in Docker.
start-docker:
    ./scripts/start.sh --docker

stop-host:
    ./scripts/stop-host.sh

stop *args:
    ./scripts/stop.sh {{args}}
