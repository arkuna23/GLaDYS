FROM node:22-bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl wget tar gzip unzip bash git \
        python3 python3-pip python3-venv python3-dev \
        build-essential pkg-config \
        jq ripgrep fd-find \
    && ln -sf /usr/bin/python3 /usr/local/bin/python \
    && ln -sf /usr/bin/fdfind /usr/local/bin/fd \
    && npm install -g @earendil-works/pi-coding-agent pi-acp pi-mcp-adapter \
    && command -v pi \
    && command -v pi-acp \
    && rm -rf /var/lib/apt/lists/*
COPY docker/entrypoint.sh /entrypoint.sh
COPY docker/daemon.toml /etc/gladys/daemon.toml
RUN chmod +x /entrypoint.sh
WORKDIR /workspace
ENV HOME=/root
ENTRYPOINT ["/entrypoint.sh"]
