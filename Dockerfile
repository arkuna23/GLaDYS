FROM node:22-bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl wget tar gzip unzip bash git \
        python3 python3-pip python3-venv python3-dev \
        build-essential pkg-config \
        jq ripgrep fd-find \
    && ln -sf /usr/bin/python3 /usr/local/bin/python \
    && ln -sf /usr/bin/fdfind /usr/local/bin/fd \
    && npm install -g --ignore-scripts @earendil-works/pi-coding-agent pi-acp pi-mcp-adapter \
    && rm -rf /var/lib/apt/lists/*
COPY docker/entrypoint.sh /entrypoint.sh
COPY docker/scheduler.toml /etc/gladys/scheduler.toml
RUN chmod +x /entrypoint.sh
WORKDIR /workspace
ENV HOME=/root
ENTRYPOINT ["/entrypoint.sh"]
