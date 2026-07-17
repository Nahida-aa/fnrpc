//! End-to-end latency benchmark for fnrpc-web vs xitca-web.
//!
//! Starts each server as an external process, sends requests via reqwest,
//! records latency, and reports P50/P95/P99/RPS.
//!
//! Usage:
//!   cargo run -p benches --bin latency --release -- fnrpc-web 10000
//!   cargo run -p benches --bin latency --release -- xitca-web 10000
//!   cargo run -p benches --bin latency --release -- all 10000

use std::process::{Command, Stdio};
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

struct LatencyStats {
    p50: f64,
    p95: f64,
    p99: f64,
    avg: f64,
    rps: f64,
    errors: usize,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = ((sorted.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted[idx]
}

fn run_bench(url: &str, n: usize, label: &str) -> LatencyStats {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .pool_max_idle_per_host(0) // no connection pooling
            .build()
            .unwrap();

        let mut latencies = Vec::with_capacity(n);
        let mut errors = 0;
        let start = Instant::now();

        for _ in 0..n {
            let t0 = Instant::now();
            match client.get(url).send().await {
                Ok(resp) => {
                    let _body = resp.bytes().await.unwrap();
                    let elapsed = t0.elapsed();
                    latencies.push(elapsed.as_secs_f64() * 1000.0);
                }
                Err(_) => {
                    errors += 1;
                }
            }
        }

        let total = start.elapsed();
        let rps = n as f64 / total.as_secs_f64();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let avg = latencies.iter().sum::<f64>() / latencies.len() as f64;
        let p50 = percentile(&latencies, 50.0);
        let p95 = percentile(&latencies, 95.0);
        let p99 = percentile(&latencies, 99.0);

        eprintln!("{label:30} {n:>8} req  {rps:>8.0} RPS  avg={avg:>7.3}ms  p50={p50:>7.3}ms  p95={p95:>7.3}ms  p99={p99:>7.3}ms  err={errors}",
            label = format!("{label}"));

        LatencyStats { p50, p95, p99, avg, rps, errors }
    })
}

fn run_fnrpc_web(n: usize) {
    let port = find_free_port();
    let addr = format!("127.0.0.1:{port}");

    let mut server = Command::new("target/release/fnrpc_web_server")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start fnrpc-web server");

    wait_for_server(port, Duration::from_secs(5));

    run_bench(&format!("http://127.0.0.1:{port}/noop?input=null"), n, "fnrpc-web/noop_json");
    run_bench(&format!("http://127.0.0.1:{port}/raw_noop"), n, "fnrpc-web/noop_raw");

    let _ = server.kill();
    let _ = server.wait();
}

fn run_xitca_web(n: usize) {
    // xitca-web server binary — use the dhat_server with xitca-web feature
    let port = find_free_port();
    let addr = format!("127.0.0.1:{port}");

    // For xitca-web, we start the benchmark server via the dhat_server binary
    // which serves on port 19111 by default. We use a different approach:
    // just test against the fnrpc-web server since both use the same HTTP stack.
    eprintln!("xitca-web server not yet available as standalone binary — skipping");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let framework = args.get(1).map(|s| s.as_str()).unwrap_or_else(|| {
        eprintln!("Usage: latency [fnrpc-web|all] [n]");
        eprintln!("  (xitca-web standalone server not yet available)");
        std::process::exit(1);
    });
    let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10_000);

    // Build the server binary first if not already built
    eprintln!("Ensuring fnrpc_web_server is built...");
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "benches", "--bin", "fnrpc_web_server"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("failed to build fnrpc_web_server");
    assert!(status.success(), "build failed");

    match framework {
        "fnrpc-web" => run_fnrpc_web(n),
        "all" => {
            run_fnrpc_web(n);
        }
        _ => {
            eprintln!("Unknown framework: {framework}");
            std::process::exit(1);
        }
    }
}
