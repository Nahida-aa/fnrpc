use dhat::Alloc;

#[global_allocator]
static ALLOC: Alloc = Alloc;

#[path = "../server/mod.rs"]
mod server;

fn usage() -> ! {
    eprintln!("Usage: dhat_server [framework] [n]");
    eprintln!();
    eprintln!("Frameworks (fnrpc variants — always available):");
    eprintln!("  fnrpc-web       — fnrpc on bare xitca-http");
    eprintln!("  fnrpc-xitca     — fnrpc on xitca-web");
    eprintln!("  fnrpc-axum      — fnrpc on axum");
    #[cfg(feature = "xitca-web-plain")]
    eprintln!("  xitca-web       — plain xitca-web (no fnrpc)");
    #[cfg(feature = "axum-plain")]
    eprintln!("  axum            — plain axum (no fnrpc)");
    #[cfg(feature = "actix-web")]
    eprintln!("  actix-web       — plain actix-web (no fnrpc)");
    #[cfg(feature = "ntex")]
    eprintln!("  ntex            — plain ntex (no fnrpc)");
    eprintln!();
    eprintln!("n: number of requests per endpoint (default: 10_000)");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let framework = args.get(1).map(|s| s.as_str()).unwrap_or_else(|| usage());
    let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10_000);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    match framework {
        "fnrpc-xitca" => rt.block_on(server::fnrpc_xitca::run("fnrpc-xitca", n)),
        "fnrpc-axum" => rt.block_on(server::fnrpc_axum::run("fnrpc-axum", n)),
        #[cfg(feature = "xitca-web-plain")]
        "xitca-web" => rt.block_on(server::xitca_web::run("xitca-web", n)),
        #[cfg(feature = "axum-plain")]
        "axum" => rt.block_on(server::axum::run("axum", n)),
        #[cfg(feature = "actix-web")]
        "actix-web" => rt.block_on(server::actix::run("actix-web", n)),
        #[cfg(feature = "ntex")]
        "ntex" => rt.block_on(server::ntex::run("ntex", n)),
        _ => usage(),
    }
}
