# Benchmark Guide for fnrpc

## Available Benchmarks

### 1. Heap Allocation Analysis (dhat)

```bash
# Per-request allocation (fnrpc-web vs xitca-web)
cargo run -p benches --bin dhat_server --features xitca-web-plain -- fnrpc-web 1000
cargo run -p benches --bin dhat_server --features xitca-web-plain -- xitca-web 1000

# Detailed comparison with all scenarios
cargo run -p benches --bin dhat_compare -- fnrpc-web 500
cargo run -p benches --bin dhat_compare -- xitca-web 500

# Startup fixed allocation
cargo run -p benches --bin dhat_dispatch --features dhat-heap
```

Output: bytes/op, blocks/op per endpoint.

### 2. End-to-End Latency + Memory (latency)

```bash
# Direct execution
cargo run -p benches --bin latency --release --features reqwest -- fnrpc-web 200 3

# Podman container (accurate memory, no host interference)
./bench-container.sh fnrpc-web 200 3

# Compare both frameworks
./bench-container.sh all 200 3

# High concurrency (adjust duration)
./bench-container.sh fnrpc-web 2000 3
```

Arguments: `<framework> <max_concurrency> <duration_secs>`
- framework: `fnrpc-web` | `xitca-web` | `all`
- max_concurrency: default 200, max ~5000
- duration_secs: default 3, longer = more stable

Output columns:
- 并发, 请求数, RPS, avg(ms), p50(ms), p95(ms), p99(ms), 错误率, 内存(MB), 增量(MB)

## When Reporting Results

Always include ALL metrics for the relevant concurrency level:
- RPS
- avg latency
- p99 latency
- Memory usage (MB)
- Error rate

Example: "at 2000 concurrency: RPS=333K, avg=5.3ms, p99=18ms, mem=34MB, err=0%"

## Current Benchmark Results Summary

### Container (4 CPUs, 256MB, debian:trixie-slim)

At 200 concurrency — fnrpc-web ≈ xitca-web across all scenarios:
- RPS: ~157K (both)
- avg: ~0.5ms (both)
- p99: ~1.9ms (both)
- mem: 6-9MB (both)

At 2000 concurrency (fnrpc-web only, same constraints):
- RPS: ~100K, avg: ~4.5ms, p99: ~14ms, mem: ~20MB

### Native (16 CPUs, no memory limit)

At 200 concurrency — fnrpc-web slightly ahead:
- noop_json: RPS=416K, avg=0.48ms, p99=1.87ms, mem=6MB
- echo_small: RPS=419K, avg=0.48ms, p99=1.87ms, mem=9MB
- lookup: RPS=407K, avg=0.49ms, p99=1.87ms, mem=13MB

At 2000 concurrency:
- noop_json: RPS=333K, avg=5.3ms, p99=18ms, mem=34MB
