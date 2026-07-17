//! Dhat allocation analysis for lookup endpoint: fnrpc-web vs xitca-web.
//!
//! Usage:
//!   cargo run -p benches --bin dhat_lookup --release -- fnrpc-web 1000
//!   cargo run -p benches --bin dhat_lookup --release -- xitca-web 1000

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
        ).is_ok() { return; }
        if start.elapsed() > timeout {
            panic!("Server did not start within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn run_bench(url: &str, n: usize, label: &str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all().build().unwrap();

    rt.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .pool_max_idle_per_host(0)
            .build()
            .unwrap();

        for _ in 0..n {
            let resp = client.get(url).send().await.unwrap();
            let _body = resp.bytes().await.unwrap();
        }
    });
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let framework = args.get(1).map(|s| s.as_str()).unwrap_or_else(|| {
        eprintln!("Usage: dhat_lookup [fnrpc-web|xitca-web] [n]");
        std::process::exit(1);
    });
    let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1000);

    let port = find_free_port();

    match framework {
        "fnrpc-web" => {
            let mut server = Command::new("target/release/fnrpc_web_server")
                .arg("--port").arg(port.to_string())
                .stdout(Stdio::null()).stderr(Stdio::null())
                .spawn().unwrap();
            wait_for_server(port, Duration::from_secs(5));
            run_bench(&format!("http://127.0.0.1:{port}/in?key=fnrpc"), n, "fnrpc-web/lookup");
            let _ = server.kill();
            let _ = server.wait();
        }
        "xitca-web" => {
            let mut server = Command::new("target/release/xitca_web_server")
                .arg(port.to_string())
                .stdout(Stdio::null()).stderr(Stdio::null())
                .spawn().unwrap();
            wait_for_server(port, Duration::from_secs(5));
            run_bench(&format!("http://127.0.0.1:{port}/in?key=fnrpc"), n, "xitca-web/lookup");
            let _ = server.kill();
            let _ = server.wait();
        }
        _ => { eprintln!("Unknown"); std::process::exit(1); }
    }
}
