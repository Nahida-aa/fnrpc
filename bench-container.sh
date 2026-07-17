#!/bin/sh
# Run latency benchmark inside a Podman container for accurate memory measurement.
#
# Uses pre-built binaries from host target/release/ — no rebuild inside container.
#
# Usage:
#   ./bench-container.sh fnrpc-web 200 3
#   ./bench-container.sh xitca-web 200 3
#   ./bench-container.sh all 200 3

set -e

FRAMEWORK="${1:-fnrpc-web}"
MAX_CONCURRENCY="${2:-200}"
DURATION="${3:-3}"
IMAGE="fnrpc-bench:latest"

# Ensure binaries are built
echo "Building binaries..."
cargo build --release -p benches \
    --bin fnrpc_web_server \
    --bin xitca_web_server \
    --bin latency --features reqwest 2>&1 | tail -3

# Build minimal container image with pre-built binaries
if ! podman image exists "$IMAGE" 2>/dev/null; then
    echo "Building container image..."
    podman build -t "$IMAGE" -f- . <<'CONTAINERFILE'
FROM docker.1ms.run/library/rust:slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl libssl3 && rm -rf /var/lib/apt/lists/*
COPY target/release/fnrpc_web_server /usr/local/bin/
COPY target/release/xitca_web_server /usr/local/bin/
COPY target/release/latency /usr/local/bin/
CMD ["latency"]
CONTAINERFILE
fi

echo ""
echo "=== Running benchmark: $FRAMEWORK (concurrency=$MAX_CONCURRENCY, duration=${DURATION}s) ==="
echo ""

podman run --rm \
    --memory=256m \
    --cpus=4 \
    --network=host \
    --name fnrpc-bench \
    "$IMAGE" \
    latency "$FRAMEWORK" "$MAX_CONCURRENCY" "$DURATION"
