# syntax=docker/dockerfile:1.4
# BharatCode CLI Docker Image
# Multi-stage build for minimal final image size

# Build stage
FROM rust:1.82-bookworm AS builder

# Install build dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    pkg-config \
    libssl-dev \
    libdbus-1-dev \
    libclang-dev \
    protobuf-compiler \
    libprotobuf-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /build

# Copy source code
COPY . .

# Build release binaries with optimizations
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ENV CARGO_PROFILE_RELEASE_LTO=true
ENV CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
ENV CARGO_PROFILE_RELEASE_OPT_LEVEL=z
ENV CARGO_PROFILE_RELEASE_STRIP=true
RUN cargo build --release --package goose-cli

# Runtime stage - minimal Debian
FROM debian:bookworm-slim@sha256:b1a741487078b369e78119849663d7f1a5341ef2768798f7b7406c4240f86aef

# Install only runtime dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    libdbus-1-3 \
    libgomp1 \
    libxcb1 \
    curl \
    git \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /build/target/release/bharatcode /usr/local/bin/bharatcode

# Create non-root user
RUN useradd -m -u 1000 -s /bin/bash bharatcode && \
    mkdir -p /home/bharatcode/.config/bharatcode && \
    chown -R bharatcode:bharatcode /home/bharatcode

# Set up environment
ENV PATH="/usr/local/bin:${PATH}"
ENV HOME="/home/bharatcode"

# Switch to non-root user
USER bharatcode
WORKDIR /home/bharatcode

# Default to BharatCode CLI
ENTRYPOINT ["/usr/local/bin/bharatcode"]
CMD ["--help"]

# Labels for metadata
LABEL org.opencontainers.image.title="bharatcode"
LABEL org.opencontainers.image.description="BharatCode CLI"
LABEL org.opencontainers.image.vendor="BharatCode"
LABEL org.opencontainers.image.source="https://github.com/aaif-bharatcode/bharatcode"
