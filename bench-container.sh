#!/bin/sh
# Run latency benchmark inside a Podman container for accurate memory measurement.
#
# Usage:
#   ./bench-container.sh fnrpc-web 200 3
#   ./bench-container.sh xitca-web 200 3
#   ./bench-container.sh all 200 3

set -e

FRAMEWORK="${1:-fnrpc-web}"
MAX_CONCURRENCY="${2:-200}"
DURATION="${3:-3}"

# Build the container image if not present
if ! podman image exists fnrpc-bench 2>/dev/null; then
    echo "Building container image..."
    podman build -t fnrpc-bench -f- . <<'CONTAINERFILE'
FROM docker.io/rust:latest AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p benches \
    --bin fnrpc_web_server \
    --bin xitca_web_server \
    --bin latency --features reqwest

FROM docker.io/rust:slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/fnrpc_web_server /usr/local/bin/
COPY --from=builder /app/target/release/xitca_web_server /usr/local/bin/
COPY --from=builder /app/target/release/latency /usr/local/bin/
CMD ["latency"]
CONTAINERFILE
fi

echo "=== Running benchmark: $FRAMEWORK (concurrency=$MAX_CONCURRENCY, duration=${DURATION}s) ==="
echo ""

podman run --rm \
    --memory=256m \
    --cpus=4 \
    --network=host \
    --name fnrpc-bench \
    fnrpc-bench \
    latency "$FRAMEWORK" "$MAX_CONCURRENCY" "$DURATION"
