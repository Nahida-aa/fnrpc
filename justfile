# fnrpc benchmark justfile
#
# Usage:
#   just bench fnrpc-web 200 3          # all endpoints
#   just bench fnrpc-web 200 3 json_te  # filter by endpoint label
#   just bench-all 200 3                # all frameworks
#   just bench-all 200 3 json_te        # all frameworks, filtered
#   just shell                          # interactive shell in container

container := "localhost/fnrpc-bench:latest"
mem := "2g"
cpus := "4"
port := "0.0.0.0:0"

# Build Rust binaries
build:
    cargo build --release -p benches \
        --bin fnrpc_web_server \
        --bin xitca_web_server \
        --bin actix_web_server --features actix-web \
        --bin latency --features reqwest

# Build container image (one-time)
image:
    podman build -t {{container}} -f Containerfile .

# Run benchmark for one framework
bench framework="fnrpc-web" concurrency="200" duration="3" *filter:
    @just build
    @just _ensure-image
    podman rm -f fnrpc-bench 2>/dev/null; true
    podman run --rm \
        --memory={{mem}} --cpus={{cpus}} --network=host \
        -v "$(pwd)/target/release/fnrpc_web_server:/usr/local/bin/fnrpc_web_server:Z" \
        -v "$(pwd)/target/release/xitca_web_server:/usr/local/bin/xitca_web_server:Z" \
        -v "$(pwd)/target/release/actix_web_server:/usr/local/bin/actix_web_server:Z" \
        -v "$(pwd)/target/release/latency:/usr/local/bin/latency:Z" \
        -e FNRPC_BIN_FNRPC_WEB=/usr/local/bin/fnrpc_web_server \
        -e FNRPC_BIN_XITCA_WEB=/usr/local/bin/xitca_web_server \
        -e FNRPC_BIN_ACTIX_WEB=/usr/local/bin/actix_web_server \
        -e FNRPC_SKIP_BUILD=1 \
        --name fnrpc-bench \
        {{container}} \
        /usr/local/bin/latency {{framework}} {{concurrency}} {{duration}} {{filter}}

# Run all three frameworks sequentially
bench-all concurrency="200" duration="3" *filter:
    @just build
    @just _ensure-image
    @just bench fnrpc-web {{concurrency}} {{duration}} {{filter}}
    @echo ""
    @just bench xitca-web {{concurrency}} {{duration}} {{filter}}
    @echo ""
    @just bench actix-web {{concurrency}} {{duration}} {{filter}}

# Interactive shell in container (for debugging)
shell:
    @just _ensure-image
    podman run --rm -it \
        --memory={{mem}} --cpus={{cpus}} --network=host \
        -v "$(pwd)/target/release:/usr/local/bin:Z" \
        --entrypoint=/bin/bash \
        {{container}}

# Ensure container image exists
_ensure-image:
    podman image exists {{container}} || podman build -t {{container}} -f Containerfile .
