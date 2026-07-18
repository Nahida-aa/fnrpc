#!/bin/bash
# Start benchmark server with CPU pinning, run latency on host.
# Usage: bench-server fnrpc-web 200 3 json_te

set -eux

FRAMEWORK="${1:-fnrpc-web}"
CONCURRENCY="${2:-200}"
DURATION="${3:-3}"
FILTER="${4:-}"

cd "$(dirname "$0")/.."

# Map framework to binary and args
case "$FRAMEWORK" in
    fnrpc-web) BIN=fnrpc_web_server; PORT=19199; ARGS="--port";;
    xitca-web) BIN=xitca_web_server; PORT=19199; ARGS="";;
    actix-web) BIN=actix_web_server; PORT=19199; ARGS="";;
esac

# Kill any previous instance
pkill -f "target/release/$BIN" 2>/dev/null || true
sleep 1

# Start server pinned to CPUs 0-3 (4 cores), rest 12 cores free for client
taskset -c 0-3 "target/release/$BIN" $ARGS $PORT &
SERVER_PID=$!
sleep 2

# Run latency on host (all 16 CPUs)
FNRPC_SKIP_BUILD=1 "target/release/latency" "$FRAMEWORK" "$CONCURRENCY" "$DURATION" "$FILTER"

# Cleanup
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true
