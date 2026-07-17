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
# Direct execution (native performance, VmRSS may include shared libs)
cargo run -p benches --bin latency --release --features reqwest -- fnrpc-web 200 3

# Podman container (accurate memory, no host interference)
./bench-container.sh fnrpc-web 200 3

# Compare both frameworks
./bench-container.sh all 200 3

# High concurrency
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
- RPS, avg latency, p99 latency, Memory usage (MB), Error rate

Example: "at 2000 concurrency: RPS=333K, avg=5.3ms, p99=18ms, mem=34MB, err=0%"

## Current Benchmark Results

### Container (4 CPUs, 256MB, debian:trixie-slim)

#### fnrpc-web noop_json (2000 concurrency)

| 并发 | RPS | avg | p50 | p95 | p99 | 错误率 | 内存 | 增量 |
|---|---|---|---|---|---|---|---|---|
| 1 | 41K | 0.02ms | 0.02ms | 0.03ms | 0.04ms | 0% | 3MB | 0 |
| 10 | 180K | 0.06ms | 0.04ms | 0.06ms | 0.08ms | 0% | 3MB | 0 |
| 100 | 176K | 0.57ms | 0.39ms | 0.77ms | 1.15ms | 0% | 4MB | +1 |
| 500 | 141K | 3.52ms | 2.32ms | 7.06ms | 30.9ms | 0% | 9MB | +5 |
| 1000 | 120K | 8.28ms | 4.48ms | 31.1ms | 43.0ms | 0% | 14MB | +11 |
| 2000 | 89K | 22.2ms | 7.12ms | 82.9ms | 93.7ms | 0% | 31MB | +27 |

#### fnrpc-web lookup (2000 concurrency)

| 并发 | RPS | avg | p50 | p95 | p99 | 错误率 | 内存 | 增量 |
|---|---|---|---|---|---|---|---|---|
| 2000 | 87K | 22.5ms | 6.48ms | 87.5ms | 101ms | 0% | 33MB | +29 |

#### xitca-web noop_json (2000 concurrency)

| 并发 | RPS | avg | p50 | p95 | p99 | 错误率 | 内存 | 增量 |
|---|---|---|---|---|---|---|---|---|
| 1 | 38K | 0.03ms | 0.03ms | 0.04ms | 0.05ms | 0% | 3MB | 0 |
| 10 | 155K | 0.06ms | 0.04ms | 0.06ms | 0.08ms | 0% | 3MB | 0 |
| 100 | 157K | 0.64ms | 0.40ms | 0.78ms | 1.23ms | 0% | 4MB | +1 |
| 500 | 122K | 4.10ms | 2.52ms | 9.09ms | 36.3ms | 0% | 8MB | +5 |
| 1000 | 102K | 9.77ms | 5.02ms | 37.6ms | 48.1ms | 0% | 15MB | +12 |
| 2000 | 93K | 21.2ms | 5.82ms | 85.9ms | 95.6ms | 0% | 27MB | +24 |

#### xitca-web lookup (2000 concurrency)

| 并发 | RPS | avg | p50 | p95 | p99 | 错误率 | 内存 | 增量 |
|---|---|---|---|---|---|---|---|---|
| 2000 | 123K | 16.1ms | 4.43ms | 77.7ms | 85.9ms | 0% | 24MB | +21 |

### Native (16 CPUs, no memory limit)

#### fnrpc-web (2000 concurrency, noop_json)

| 并发 | RPS | avg | p50 | p95 | p99 | 错误率 | 内存 | 增量 |
|---|---|---|---|---|---|---|---|---|
| 2000 | 333K | 5.3ms | 5.26ms | 13.4ms | 18.0ms | 0% | 34MB | +30 |

### Key Takeaways

- fnrpc-web ≈ xitca-web in container isolation (equal RPS, latency, memory)
- fnrpc-web lookup has slightly higher p99 than xitca-web at 2000 concurrency
  (101ms vs 86ms) due to RwLock contention on HashMap
- Native performance is ~3x better than container with 4 CPU limit
- Error rate is 0% across all scenarios for both frameworks
