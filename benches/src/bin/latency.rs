//! Concurrent latency benchmark for fnrpc-web vs xitca-web.
//!
//! Reference: tt benchmark (web-server/tt) — ramp-up concurrency, measure
//! RPS, latency percentiles, and error rate at each level.
//!
//! Usage:
//!   cargo run -p benches --bin latency --release -- fnrpc-web 10000
//!   cargo run -p benches --bin latency --release -- all 10000

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
        ).is_ok() {
            return;
        }
        if start.elapsed() > timeout {
            panic!("Server did not start within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct BenchResult {
    concurrency: usize,
    duration: Duration,
    requests: usize,
    errors: usize,
    latencies: Vec<f64>, // ms
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = ((sorted.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted[idx]
}

/// Run a benchmark at a specific concurrency level.
/// Spawns `concurrency` tasks, each sending requests in a loop for `duration`.
async fn bench_concurrent(
    url: &str,
    concurrency: usize,
    duration: Duration,
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
        let url = url.to_string();

        handles.push(tokio::spawn(async move {
            loop {
                if stop.load(Ordering::Relaxed) { break; }
                let t0 = Instant::now();
                match client.get(&url).send().await {
                    Ok(resp) => {
                        let _body = resp.bytes().await.unwrap();
                        let elapsed = t0.elapsed();
                        latencies.lock().unwrap().push(elapsed.as_secs_f64() * 1000.0);
                    }
                    Err(_) => {
                        *errors.lock().unwrap() += 1;
                    }
                }
            }
        }));
    }

    tokio::time::sleep(duration).await;
    stop.store(true, Ordering::Relaxed);

    // Wait for all tasks to stop
    for h in handles {
        let _ = h.await;
    }

    let latencies = latencies.lock().unwrap().clone();
    let errors = *errors.lock().unwrap();

    BenchResult {
        concurrency,
        duration,
        requests: latencies.len(),
        errors,
        latencies,
    }
}

fn print_results(scenario: &str, results: &[BenchResult]) {
    println!();
    println!("  {scenario}");
    println!("  {:>6}  {:>8}  {:>8}  {:>8}  {:>8}  {:>8}  {:>8}  {:>8}",
        "并发", "请求数", "RPS", "avg(ms)", "p50(ms)", "p95(ms)", "p99(ms)", "错误率");
    println!("  {}",
        std::iter::repeat("-").take(80).collect::<String>());

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

        println!("  {:>6}  {:>8}  {:>8.0}  {:>8.3}  {:>8.3}  {:>8.3}  {:>8.3}  {:>7.2}%",
            r.concurrency, r.requests, rps, avg, p50, p95, p99, err_rate);
    }
}

async fn run_scenario(url: &str, scenario: &str, concurrency_levels: &[usize], duration: Duration) {
    let mut results = Vec::new();
    for &c in concurrency_levels {
        eprintln!("  {scenario} concurrency={c}...");
        let result = bench_concurrent(url, c, duration).await;
        results.push(result);
    }
    print_results(scenario, &results);
}

fn run_fnrpc_web(concurrency_levels: &[usize], duration: Duration) {
    let port = find_free_port();

    let mut server = Command::new("target/release/fnrpc_web_server")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start fnrpc-web server");

    wait_for_server(port, Duration::from_secs(5));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        run_scenario(
            &format!("http://127.0.0.1:{port}/noop?input=null"),
            "fnrpc-web/noop_json",
            concurrency_levels,
            duration,
        ).await;
        run_scenario(
            &format!("http://127.0.0.1:{port}/raw_noop"),
            "fnrpc-web/noop_raw",
            concurrency_levels,
            duration,
        ).await;
        run_scenario(
            &format!("http://127.0.0.1:{port}/in?key=fnrpc"),
            "fnrpc-web/lookup",
            concurrency_levels,
            duration,
        ).await;
    });

    let _ = server.kill();
    let _ = server.wait();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let framework = args.get(1).map(|s| s.as_str()).unwrap_or_else(|| {
        eprintln!("Usage: latency [fnrpc-web|all] [max_concurrency] [duration_secs]");
        eprintln!("  Measures RPS, latency percentiles, and error rate at increasing concurrency levels");
        std::process::exit(1);
    });
    let max_concurrency: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1000);
    let duration_secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2);

    // Ramp-up concurrency levels: 1, 10, 50, 100, 200, 500, 1000, ...
    let mut levels = vec![1, 10, 50, 100];
    let mut c = 200;
    while c <= max_concurrency {
        levels.push(c);
        c *= 2;
    }
    if *levels.last().unwrap() != max_concurrency {
        levels.push(max_concurrency);
    }

    let duration = Duration::from_secs(duration_secs);

    eprintln!("Building fnrpc_web_server...");
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "benches", "--bin", "fnrpc_web_server"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("failed to build fnrpc_web_server");
    assert!(status.success(), "build failed");

    match framework {
        "fnrpc-web" => {
            println!("fnrpc-web 并发基准测试 (每级 {duration_secs} 秒)");
            run_fnrpc_web(&levels, duration);
        }
        "all" => {
            println!("fnrpc-web 并发基准测试 (每级 {duration_secs} 秒)");
            run_fnrpc_web(&levels, duration);
        }
        _ => {
            eprintln!("Unknown framework: {framework}");
            std::process::exit(1);
        }
    }
}
