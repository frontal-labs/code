FROM rust:1.95-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1 AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        git \
        libssl-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --locked --release -p code-server

FROM debian:bookworm-slim@sha256:4724b8cc51e33e398f0e2e15e18d5ec2851ff0c2280647e1310bc1642182655d

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        docker.io \
        git \
        libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash frontal-code

WORKDIR /workspace

COPY --from=builder /src/target/release/code-server /usr/local/bin/code-server

RUN mkdir -p /workspace /var/lib/frontal-code/server /var/lib/frontal-code/agents \
    && chown -R frontal-code:frontal-code /workspace /var/lib/frontal-code

USER frontal-code

EXPOSE 8788

CMD ["code-server"]
