#!/bin/bash
set -e

PORT=19111
SERVER_PID=""

cleanup() {
    if [ -n "$SERVER_PID" ]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Build the server
echo "Building fnrpc-web server..."
cargo build --release -p benches --bin fnrpc_web_server 2>&1 | tail -3

# Start the server in background
echo "Starting fnrpc-web server on port $PORT..."
./target/release/fnrpc_web_server &
SERVER_PID=$!

# Wait for server to be ready
echo "Waiting for server to be ready..."
for i in $(seq 1 30); do
    if curl -s "http://localhost:$PORT/fnrpc/noop?input=null" > /dev/null 2>&1; then
        echo "Server is ready!"
        break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "Server crashed!"
        exit 1
    fi
    sleep 0.5
done

echo ""
echo "=== Latency Benchmarks (100 runs) ==="
echo ""

echo "--- fnrpc-web GET /fnrpc/noop ---"
hyperfine --warmup 5 --runs 100 \
    "curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null" \
    --export-json fnrpc-web-get-noop.json \
    2>&1 | grep -E 'Mean|Median|Time'

echo ""
echo "--- fnrpc-web GET /fnrpc/echo ---"
hyperfine --warmup 5 --runs 100 \
    "curl -s 'http://localhost:$PORT/fnrpc/echo?input=%22hello%22' > /dev/null" \
    --export-json fnrpc-web-get-echo.json \
    2>&1 | grep -E 'Mean|Median|Time'

echo ""
echo "--- fnrpc-web POST /fnrpc/echo (JSON body) ---"
hyperfine --warmup 5 --runs 100 \
    "curl -s -X POST -H 'Content-Type: application/json' -d '\"hello\"' http://localhost:$PORT/fnrpc/echo > /dev/null" \
    --export-json fnrpc-web-post-echo.json \
    2>&1 | grep -E 'Mean|Median|Time'

echo ""
echo "=== Throughput Benchmarks (concurrent curl) ==="
echo ""

echo "--- fnrpc-web GET /fnrpc/noop (concurrent) ---"
hyperfine --warmup 3 --runs 3 \
    "curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null &
     curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null &
     curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null &
     curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null &
     curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null &
     curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null &
     curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null &
     curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null &
     curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null &
     curl -s http://localhost:$PORT/fnrpc/noop?input=null > /dev/null &
     wait" \
    2>&1 | grep -E 'Mean|Median|Time'

echo ""
echo "=== Done ==="
