set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @just --list

# Generate workspace configs (skip files that already exist).
init:
    ./scripts/init.sh


# Shell into the agent container.
shell:
    ./scripts/shell.sh

# Start docker agent+scheduler and host Channel/Memory/Gateway.
start:
    ./scripts/start.sh

stop-host:
    ./scripts/stop-host.sh

stop:
    ./scripts/stop.sh
