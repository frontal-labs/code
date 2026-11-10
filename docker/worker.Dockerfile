FROM rust:1.95-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1 AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        git \
        libpq-dev \
        libsqlite3-dev \
        libssl-dev \
        pkg-config \
        sqlite3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src

# The workspace manifest lists every member, so the whole tree is needed to resolve it.
# .dockerignore keeps the context to the Rust workspace.
COPY . .

RUN cargo build --locked --release -p cli

FROM debian:bookworm-slim@sha256:4724b8cc51e33e398f0e2e15e18d5ec2851ff0c2280647e1310bc1642182655d

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        build-essential \
        ca-certificates \
        curl \
        git \
        libpq5 \
        libsqlite3-0 \
        libssl3 \
        postgresql-client \
        python3 \
        python3-pip \
        sqlite3 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash frontal-code

WORKDIR /workspace

# The package builds two commands for the same CLI: `code`, and `frontal-code`, which runs the
# `code` beside it. The server launches `frontal-code hosted task run`, so both are installed.
COPY --from=builder /src/target/release/code /usr/local/bin/code
COPY --from=builder /src/target/release/frontal-code /usr/local/bin/frontal-code

RUN chmod +x /usr/local/bin/code /usr/local/bin/frontal-code \
    && mkdir -p /workspace/.frontal-code /workspace/.sandbox-home \
    && chown -R frontal-code:frontal-code /workspace

USER frontal-code

ENV CARGO_TERM_COLOR=always
ENV FRONTAL_CODE_HOME=/workspace/.frontal-code
ENV SANDBOX_HOME=/workspace/.sandbox-home

CMD ["code", "--version"]
