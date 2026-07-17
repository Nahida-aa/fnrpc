//! Concurrent latency benchmark for fnrpc-web vs xitca-web.
//!
//! Reference: tt benchmark (web-server/tt) — ramp-up concurrency, measure
//! RPS, latency percentiles, error rate, and memory usage at each level.
//! Supports GET and POST scenarios with configurable payload sizes.
//!
//! Usage:
//!   cargo run -p benches --bin latency --release -- fnrpc-web 2000 3
//!   cargo run -p benches --bin latency --release -- all 2000 3

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::net::TcpListener;

fn find_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn wait_for_server(port: u16, timeout: Duration) {
    let start = Instant::now();
    loop {
        if std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(50),
        ).is_ok() { return; }
        if start.elapsed() > timeout {
            panic!("Server did not start within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Read process memory (VmRSS from /proc/<pid>/status).
/// For more accurate measurement (excluding shared library duplicates),
/// run the server inside a Docker/Podman container.
fn read_memory_kb(pid: u32) -> u64 {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    for line in status.lines() {
        if let Some(rss) = line.strip_prefix("VmRSS:") {
            if let Some(kb) = rss.trim().strip_suffix(" kB") {
                return kb.trim().parse().unwrap_or(0);
            }
        }
    }
    0
}

struct BenchResult {
    concurrency: usize,
    duration: Duration,
    requests: usize,
    errors: usize,
    latencies: Vec<f64>,
    mem_kb: u64,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = ((sorted.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted[idx]
}

enum Req {
    Get(String),
    Post(String, Vec<u8>),
}

async fn bench_concurrent(
    req: &Req, concurrency: usize, duration: Duration,
) -> BenchResult {
    let stop = Arc::new(AtomicBool::new(false));
    let latencies = Arc::new(std::sync::Mutex::new(Vec::new()));
    let errors = Arc::new(std::sync::Mutex::new(0usize));

    let client = reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(usize::MAX)
        .build()
        .unwrap();

    let mut handles = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let stop = stop.clone();
        let latencies = latencies.clone();
        let errors = errors.clone();
        let client = client.clone();
        let req = match req {
            Req::Get(url) => Req::Get(url.clone()),
            Req::Post(url, body) => Req::Post(url.clone(), body.clone()),
        };
        handles.push(tokio::spawn(async move {
            loop {
                if stop.load(Ordering::Relaxed) { break; }
                let t0 = Instant::now();
                let result = match &req {
                    Req::Get(url) => client.get(url).send().await,
                    Req::Post(url, body) => client.post(url)
                        .header("content-type", "application/json")
                        .body(body.clone())
                        .send().await,
                };
                match result {
                    Ok(resp) => {
                        let _body = resp.bytes().await.unwrap();
                        latencies.lock().unwrap().push(t0.elapsed().as_secs_f64() * 1000.0);
                    }
                    Err(_) => { *errors.lock().unwrap() += 1; }
                }
            }
        }));
    }

    tokio::time::sleep(duration).await;
    stop.store(true, Ordering::Relaxed);
    for h in handles { let _ = h.await; }

    let latencies = latencies.lock().unwrap().clone();
    let errors = *errors.lock().unwrap();
    BenchResult { concurrency, duration, requests: latencies.len(), errors, latencies, mem_kb: 0 }
}

fn print_header() {
    println!("  {:>6}  {:>8}  {:>10}  {:>8}  {:>8}  {:>8}  {:>8}  {:>7}  {:>8}  {:>8}",
        "并发", "请求数", "RPS", "avg(ms)", "p50(ms)", "p95(ms)", "p99(ms)", "错误率", "内存(MB)", "增量(MB)");
    println!("  {}",
        std::iter::repeat("-").take(108).collect::<String>());
}

fn print_results(scenario: &str, results: &[BenchResult], baseline_kb: u64) {
    println!();
    println!("  {scenario}  (基线: {} MB)", baseline_kb / 1024);
    print_header();
    for r in results {
        let mut sorted = r.latencies.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let avg = sorted.iter().sum::<f64>() / sorted.len().max(1) as f64;
        let p50 = percentile(&sorted, 50.0);
        let p95 = percentile(&sorted, 95.0);
        let p99 = percentile(&sorted, 99.0);
        let rps = r.requests as f64 / r.duration.as_secs_f64();
        let err_rate = if r.requests + r.errors > 0 {
            r.errors as f64 / (r.requests + r.errors) as f64 * 100.0
        } else { 0.0 };
        let delta = r.mem_kb as i64 - baseline_kb as i64;
        println!("  {:>6}  {:>8}  {:>10.0}  {:>8.3}  {:>8.3}  {:>8.3}  {:>8.3}  {:>6.2}%  {:>8}  {:>+9}",
            r.concurrency, r.requests, rps, avg, p50, p95, p99, err_rate, r.mem_kb / 1024, delta / 1024);
    }
}

async fn run_scenario(
    req: &Req, scenario: &str,
    concurrency_levels: &[usize], duration: Duration,
    server_pid: u32, baseline_kb: u64,
) {
    let mut results = Vec::new();
    for &c in concurrency_levels {
        eprintln!("  {scenario} concurrency={c}...");
        let mut result = bench_concurrent(req, c, duration).await;
        result.mem_kb = read_memory_kb(server_pid);
        results.push(result);
    }
    print_results(scenario, &results, baseline_kb);
}

fn run_framework(
    name: &str, default_binary: &str,
    concurrency_levels: &[usize], duration: Duration,
    endpoints: &[(&str, &str, bool, &[u8])],
) {
    // Allow overriding binary path via env var (used in container)
    let env_key = format!("FNRPC_BIN_{}", name.to_uppercase().replace('-', "_"));
    let binary = std::env::var(&env_key).unwrap_or_else(|_| default_binary.to_string());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all().build().unwrap();

    rt.block_on(async {
        for (path, label, is_post, body) in endpoints {
            let port = find_free_port();
            let mut server = if name == "xitca-web" {
                Command::new(&binary).arg(port.to_string())
                    .stdout(Stdio::null()).stderr(Stdio::null()).spawn()
            } else {
                Command::new(&binary).arg("--port").arg(port.to_string())
                    .stdout(Stdio::null()).stderr(Stdio::null()).spawn()
            };
            let mut server = server.expect(&format!("failed to start {name} server"));
            wait_for_server(port, Duration::from_secs(5));
            let server_pid = server.id();
            let baseline_mem = read_memory_kb(server_pid);
            eprintln!("  {name}/{label} baseline: {} MB", baseline_mem / 1024);

            let req = if *is_post {
                Req::Post(format!("http://127.0.0.1:{port}{path}"), body.to_vec())
            } else {
                Req::Get(format!("http://127.0.0.1:{port}{path}"))
            };
            run_scenario(&req, &format!("{name}/{label}"),
                concurrency_levels, duration, server_pid, baseline_mem).await;

            let _ = server.kill();
            let _ = server.wait();
        }
    });
}

fn build_server(name: &str) {
    // Allow skipping build via env var (used in container: binaries are pre-built)
    if std::env::var("FNRPC_SKIP_BUILD").is_ok() {
        return;
    }
    eprintln!("Building {name}...");
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "benches", "--bin", name])
        .stdout(Stdio::null()).stderr(Stdio::null())
        .status()
        .expect(&format!("failed to build {name}"));
    assert!(status.success(), "build failed");
}

fn concurrency_levels(max: usize) -> Vec<usize> {
    let mut levels = vec![1, 10, 50, 100, 200, 500];
    let mut c = 1000;
    while c <= max {
        levels.push(c);
        if c >= 2000 { c += 2000; } else { c += 1000; }
    }
    if *levels.last().unwrap() != max { levels.push(max); }
    levels
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let framework = args.get(1).map(|s| s.as_str()).unwrap_or_else(|| {
        eprintln!("Usage: latency [fnrpc-web|xitca-web|all] [max_concurrency] [duration_secs]");
        std::process::exit(1);
    });
    let max_concurrency: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let duration_secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);
    let levels = concurrency_levels(max_concurrency);
    let duration = Duration::from_secs(duration_secs);

    // Pre-built JSON payloads for POST benchmarks
    let medium_body = br#"{"id":42,"name":"Alice Johnson","email":"alice@example.com","tags":["premium","vip","early-adopter"],"score":98.5}"#;
    let large_body = br#"{"items":[{"id":0,"name":"product-0","description":"A high-quality item with excellent features and durable construction suitable for various uses.","price":19.99,"quantity":100,"category":"electronics","tags":["new","popular","discount"],"metadata":{"color":"red","size":"XL","weight":"1.5kg"}},{"id":1,"name":"product-1","description":"A high-quality item with excellent features and durable construction suitable for various uses.","price":20.99,"quantity":101,"category":"electronics","tags":["new","popular","discount"],"metadata":{"color":"red","size":"XL","weight":"1.5kg"}}]}"#;

    // (path, label, is_post, body)
    let fnrpc_endpoints: &[(&str, &str, bool, &[u8])] = &[
        ("/noop?input=null", "noop_json", false, b""),
        ("/raw_noop", "noop_raw", false, b""),
        ("/in?key=fnrpc", "lookup", false, b""),
        ("/echo", "echo_small", true, br#""hello""#),
        ("/medium", "echo_medium", true, medium_body),
        ("/large", "echo_large", true, large_body),
    ];

    let xitca_endpoints: &[(&str, &str, bool, &[u8])] = &[
        ("/noop-json", "noop_json", false, b""),
        ("/noop-raw", "noop_raw", false, b""),
        ("/echo", "echo_small", true, br#""hello""#),
        ("/medium", "echo_medium", true, medium_body),
        ("/large", "echo_large", true, large_body),
        ("/in?key=fnrpc", "lookup", false, b""),
    ];

    match framework {
        "fnrpc-web" => {
            build_server("fnrpc_web_server");
            println!("fnrpc-web (每级 {duration_secs} 秒, 最大 {max_concurrency} 并发)");
            run_framework("fnrpc-web", "target/release/fnrpc_web_server",
                &levels, duration, fnrpc_endpoints);
        }
        "xitca-web" => {
            build_server("xitca_web_server");
            println!("xitca-web (每级 {duration_secs} 秒, 最大 {max_concurrency} 并发)");
            run_framework("xitca-web", "target/release/xitca_web_server",
                &levels, duration, xitca_endpoints);
        }
        "all" => {
            build_server("fnrpc_web_server");
            build_server("xitca_web_server");
            println!("fnrpc-web (每级 {duration_secs} 秒, 最大 {max_concurrency} 并发)");
            run_framework("fnrpc-web", "target/release/fnrpc_web_server",
                &levels, duration, fnrpc_endpoints);
            println!("\nxitca-web (每级 {duration_secs} 秒, 最大 {max_concurrency} 并发)");
            run_framework("xitca-web", "target/release/xitca_web_server",
                &levels, duration, xitca_endpoints);
        }
        _ => { eprintln!("Unknown: {framework}"); std::process::exit(1); }
    }
}
