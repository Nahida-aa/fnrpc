#!/bin/sh
# Run latency benchmark inside a Podman container for accurate memory measurement.
#
# Builds a container image from debian:bookworm-slim (glibc-compatible).
# Falls back to direct execution when no network is available.
#
# Usage:
#   ./bench-container.sh fnrpc-web 200 3
#   ./bench-container.sh xitca-web 200 3
#   ./bench-container.sh all 200 3

set -e

FRAMEWORK="${1:-fnrpc-web}"
MAX_CONCURRENCY="${2:-200}"
DURATION="${3:-3}"
IMAGE="localhost/fnrpc-bench:latest"

# Build binaries on host (fast incremental builds)
echo "Building binaries..."
cargo build --release -p benches \
    --bin fnrpc_web_server \
    --bin xitca_web_server \
    --bin actix_web_server --features actix-web \
    --bin latency --features reqwest 2>&1 | tail -3

# Build minimal container image once (cached)
if ! podman image exists "$IMAGE" 2>/dev/null; then
    echo "Building container image..."
    echo "Pulling debian base image..."
    if ! podman pull docker.io/library/debian:trixie-slim 2>/dev/null; then
        echo "Warning: cannot pull debian image. Falling back to direct execution."
        echo ""
        exec cargo run --release -p benches --bin latency --features reqwest -- \
            "$FRAMEWORK" "$MAX_CONCURRENCY" "$DURATION"
    fi

    cat > /tmp/Containerfile.fnrpc << 'CONTAINERFILE'
FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl && rm -rf /var/lib/apt/lists/*
CMD ["/usr/local/bin/latency"]
CONTAINERFILE
    podman build -t "$IMAGE" -f /tmp/Containerfile.fnrpc .
    rm -f /tmp/Containerfile.fnrpc
fi

echo ""
echo "=== Running benchmark: $FRAMEWORK (concurrency=$MAX_CONCURRENCY, duration=${DURATION}s) ==="
echo ""

# Mount host binaries into container (no rebuild needed for code changes)
podman run --rm \
    --memory=2g \
    --cpus=4 \
    --network=host \
    -v "$(pwd)/target/release/fnrpc_web_server:/usr/local/bin/fnrpc_web_server:Z" \
    -v "$(pwd)/target/release/xitca_web_server:/usr/local/bin/xitca_web_server:Z" \
    -v "$(pwd)/target/release/actix_web_server:/usr/local/bin/actix_web_server:Z" \
    -v "$(pwd)/target/release/latency:/usr/local/bin/latency:Z" \
    -e FNRPC_BIN_FNRPC_WEB=/usr/local/bin/fnrpc_web_server \
    -e FNRPC_BIN_XITCA_WEB=/usr/local/bin/xitca_web_server \
    -e FNRPC_BIN_ACTIX_WEB=/usr/local/bin/actix_web_server \
    -e FNRPC_SKIP_BUILD=1 \
    --name fnrpc-bench \
    --name fnrpc-bench \
    "$IMAGE" \
    /usr/local/bin/latency "$FRAMEWORK" "$MAX_CONCURRENCY" "$DURATION"
