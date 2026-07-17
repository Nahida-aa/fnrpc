#!/bin/sh
# Run latency benchmark inside a Podman container for accurate memory measurement.
#
# Builds a minimal local image from scratch — no network pull required.
# Uses host's Rust compiler to build static binaries.
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

# Build binaries
echo "Building binaries..."
cargo build --release -p benches \
    --bin fnrpc_web_server \
    --bin xitca_web_server \
    --bin latency --features reqwest 2>&1 | tail -3

# Build minimal container image (static binaries, no base image needed)
if ! podman image exists "$IMAGE" 2>/dev/null; then
    echo "Building container image..."
    # Try to pull a minimal base image (alpine is ~5MB)
    echo "Pulling alpine base image..."
    if ! podman pull docker.1ms.run/library/alpine:latest 2>/dev/null && \
       ! podman pull docker.io/library/alpine:latest 2>/dev/null; then
        echo "Warning: cannot pull alpine image. Falling back to direct execution."
        echo ""
        exec cargo run --release -p benches --bin latency --features reqwest -- \
            "$FRAMEWORK" "$MAX_CONCURRENCY" "$DURATION"
    fi

    cat > /tmp/Containerfile.fnrpc << 'CONTAINERFILE'
FROM alpine:latest
RUN apk add --no-cache libgcc libstdc++ curl ca-certificates
COPY target/release/fnrpc_web_server /usr/local/bin/
COPY target/release/xitca_web_server /usr/local/bin/
COPY target/release/latency /usr/local/bin/
CMD ["latency"]
CONTAINERFILE
    podman build -t "$IMAGE" -f /tmp/Containerfile.fnrpc .
    rm -f /tmp/Containerfile.fnrpc
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
    /latency "$FRAMEWORK" "$MAX_CONCURRENCY" "$DURATION"
