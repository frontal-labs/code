FROM rust:1.75-bookworm@sha256:87f3b2f93b82995443a1a558c234212dafe79cfdc3af956539610560369ddcd0 AS builder

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

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release -p cli

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

COPY --from=builder /src/target/release/frontal-code /usr/local/bin/frontal-code

RUN chmod +x /usr/local/bin/frontal-code \
    && mkdir -p /workspace/.frontal-code /workspace/.sandbox-home \
    && chown -R frontal-code:frontal-code /workspace

USER frontal-code

ENV CARGO_TERM_COLOR=always
ENV FRONTAL_CODE_HOME=/workspace/.frontal-code
ENV SANDBOX_HOME=/workspace/.sandbox-home

CMD ["frontal-code", "--version"]
