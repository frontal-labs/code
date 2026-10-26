# Syntax:
#   docker build -f sdks/rust/Dockerfile -t ghcr.io/frontal-labs/code/code-sdk-rs:latest .
#
# Publishes the Frontal Code Rust SDK (frontal-code-sdk) as a distributable container
# image on the GitHub Container Registry (ghcr.io). The image carries the packaged
# .crate, README, and LICENSE so consumers can vendor or inspect the exact release.

FROM rust:1.95-bookworm AS builder

WORKDIR /workspace

# The Cargo workspace lists every member, so all member directories must be
# present for manifest resolution even though only frontal-code-sdk is packaged.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo ./.cargo
COPY cli ./cli
COPY crates ./crates
COPY sdks ./sdks
COPY tests ./tests
COPY tools ./tools

RUN cargo package -p frontal-code-sdk --no-verify --allow-dirty

FROM debian:bookworm-slim@sha256:4724b8cc51e33e398f0e2e15e18d5ec2851ff0c2280647e1310bc1642182655d

LABEL org.opencontainers.image.source="https://github.com/frontal-labs/code"
LABEL org.opencontainers.image.description="Frontal Code Rust SDK (frontal-code-sdk) crate package"
LABEL org.opencontainers.image.licenses="Apache-2.0"

WORKDIR /opt/code-sdk-rs

COPY --from=builder /workspace/target/package/frontal-code-sdk-*.crate ./
COPY --from=builder /workspace/sdks/rust/README.md /workspace/sdks/rust/LICENSE.md ./

ENTRYPOINT ["ls"]
CMD ["-1", "/opt/code-sdk-rs"]
