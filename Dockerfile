# syntax=docker/dockerfile:1
#
# Multi-stage build -- produces a minimal image (~20 MB) with just the
# zamsync binary and CA certificates.
#
# ARM builds (Raspberry Pi):
#   docker buildx build --platform linux/arm64  -t zamsync:arm64  .
#   docker buildx build --platform linux/arm/v7 -t zamsync:armv7  .
#
# For faster ARM cross-compilation, use a native ARM builder node:
#   docker buildx create --name arm-builder --platform linux/arm64,linux/arm/v7
#   docker buildx use arm-builder

# ── Stage 1: build ───────────────────────────────────────────────────────────
FROM --platform=$TARGETPLATFORM rust:1-slim-bookworm AS builder

WORKDIR /src

# Fetch dependencies in a separate layer so rebuilds are fast when only
# application source changes (not Cargo.toml / Cargo.lock).
COPY Cargo.toml Cargo.lock ./
COPY crates/zamsync-core/Cargo.toml     crates/zamsync-core/Cargo.toml
COPY crates/zamsync-storage/Cargo.toml  crates/zamsync-storage/Cargo.toml
COPY crates/zamsync-network/Cargo.toml  crates/zamsync-network/Cargo.toml
COPY crates/zamsync-testing/Cargo.toml  crates/zamsync-testing/Cargo.toml

# Create stub lib/main files so `cargo fetch` can resolve the full dep graph.
RUN mkdir -p src \
        crates/zamsync-core/src \
        crates/zamsync-storage/src \
        crates/zamsync-network/src \
        crates/zamsync-testing/src && \
    echo 'fn main() {}' > src/main.rs && \
    echo '' > crates/zamsync-core/src/lib.rs && \
    echo '' > crates/zamsync-storage/src/lib.rs && \
    echo '' > crates/zamsync-network/src/lib.rs && \
    echo '' > crates/zamsync-testing/src/lib.rs

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo fetch

# Copy real source and build the release binary.
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target-v2 \
    cargo build --release && \
    cp target/release/zamsync /usr/local/bin/zamsync

# ── Stage 2: runtime ─────────────────────────────────────────────────────────
FROM --platform=$TARGETPLATFORM debian:bookworm-slim

# ca-certificates is needed for any outbound TLS the binary might make;
# also needed so rustls can verify external certs if ever added.
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd -r -g 1000 zamsync && \
    useradd  -r -u 1000 -g zamsync -s /bin/false -d /var/lib/zamsync zamsync && \
    mkdir -p /var/lib/zamsync && \
    chown zamsync:zamsync /var/lib/zamsync

COPY --from=builder /usr/local/bin/zamsync /usr/local/bin/zamsync

USER zamsync

# Persistent data: WAL, peer state, TLS credentials (node.crt, node.key, ca.crt).
VOLUME /var/lib/zamsync

# Default sync port.
EXPOSE 7000

ENTRYPOINT ["/usr/local/bin/zamsync"]

# Override with `docker run zamsync sync /data <peer-addr> <peer-id>` etc.
CMD ["serve", "/var/lib/zamsync", "0.0.0.0:7000"]
