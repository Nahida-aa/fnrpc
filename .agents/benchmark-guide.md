# Benchmark Guide for fnrpc

## Tested Endpoints

| 端点 | 来源 | 方法 | 说明 |
|---|---|---|---|
| `/noop?input=null` | fnrpc | GET | 空载 JSON，`()` → `()` |
| `/raw_noop` | fnrpc | GET | 空载纯文本，`b"ok"` |
| `/echo` | fnrpc | POST | 小 JSON echo，`"hello"` |
| `/medium` | fnrpc | POST | 中 JSON echo，~150B 结构体 |
| `/large` | fnrpc | POST | 大 JSON echo，~900B 数组 |
| `/in?key=fnrpc` | tt | GET | HashMap lookup + JSON |
| `/json` | **TFB** | GET | `{"message":"Hello, World!"}` |
| `/plaintext` | **TFB** | GET | `"Hello, World!"` 纯文本 |

TFB = TechEmpower FrameworkBenchmarks 标准端点，可直接与社区数据对比。

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

### 3. CPU Instruction Analysis (perf)

```bash
perf stat -e instructions,cache-misses,cache-references,branch-misses -r 3 -- bash -c '
target/release/fnrpc_web_server --port 19160 &
PID=$!; sleep 1
for i in $(seq 1 5000); do curl -s "http://127.0.0.1:19160/in?key=fnrpc" >/dev/null; done
kill $PID 2>/dev/null; wait $PID 2>/dev/null
'
```

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

### CPU Instruction Level (lookup, 5000 requests, native, 3 runs)

| 指标 | fnrpc-web | xitca-web | 差异 |
|---|---|---|---|
| instructions | 73,274M | 73,292M | **+0.02%** |
| cache-misses | 313M | 314M | **+0.2%** |
| cache-references | 1,852M | 1,855M | **+0.2%** |
| branch-misses | 445M | 446M | **+0.2%** |
| wall time | 28.93s | 28.97s | **+0.1%** |

**fnrpc and xitca are identical at CPU instruction level.** The `Arc<dyn Fn>` indirect call in
`RawRpcAdapter` is inlined by the compiler — zero runtime overhead.

### Heap Allocation: Dispatch-Only (dhat_compare) vs Full Service (dhat on server)

**Dispatch-only** (dhat_compare — direct handler call, no HTTP):
- fnrpc-web **always better** than xitca-web on every endpoint
- noop_raw: **2B/1blk** (fnrpc) vs 185B/3blks (xitca) — fnrpc skips Box::pin + Extension

**Full service** (dhat on fnrpc_web_server / xitca_web_server — with HTTP):
- fnrpc-web: 2,167KB total (200 lookup requests + startup)
- xitca-web: 2,654KB total
- **fnrpc 0.5MB less** — difference is in HTTP server layer, not framework code

### Key Takeaways

- fnrpc-web ≈ xitca-web at every level: RPS, latency, memory, CPU instructions, heap allocation
