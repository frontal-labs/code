# Development Dockerfile with hot reload
FROM rust:1.88-bookworm

# Install development dependencies
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        git \
        libssl-dev \
        pkg-config \
        sqlite3 \
        libsqlite3-dev \
        libpq-dev \
        curl \
        build-essential \
        postgresql-client \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-watch for hot reload
RUN cargo install cargo-watch --locked

# Set working directory
WORKDIR /workspace

# Create non-root user
RUN useradd --create-home --shell /bin/bash frontal-code

# Switch to non-root user
USER frontal-code

# Environment variables
ENV CARGO_TERM_COLOR=always
ENV FCODE_HOME=/workspace/.frontal-code
ENV SANDBOX_HOME=/workspace/.sandbox-home

# Default command with hot reload
CMD ["cargo", "watch", "-x", "run"]
