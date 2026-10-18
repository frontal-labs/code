FROM rust:1.95-bookworm AS builder

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

# The Cargo workspace lists every member, so all member directories must be
# present for manifest resolution even though only code-server is built.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo ./.cargo
COPY cli ./cli
COPY crates ./crates
COPY sdks ./sdks
COPY tests ./tests
COPY tools ./tools

RUN cargo build --release --locked -p code-server

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash frontal-code

WORKDIR /workspace

COPY --from=builder /src/target/release/code-server /usr/local/bin/code-server

RUN mkdir -p /workspace/workspaces /var/lib/frontal-code/server /var/lib/frontal-code/agents \
    && chown -R frontal-code:frontal-code /workspace /var/lib/frontal-code

USER frontal-code

EXPOSE 8788

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://127.0.0.1:8788/health || exit 1

CMD ["code-server"]
