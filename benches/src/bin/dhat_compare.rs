//! Detailed dhat allocation comparison between fnrpc-web and xitca-web (plain).
//!
//! Saves dhat JSON per benchmark phase for post-hoc call-stack analysis.
//!
//! Usage:
//!   cargo run -p benches --bin dhat_compare -- fnrpc-web [n]
//!   cargo run -p benches --bin dhat_compare -- xitca-web [n]

use benches::compare;
use dhat::Alloc;

#[global_allocator]
static ALLOC: Alloc = Alloc;

// #[path = "../compare/mod.rs"]
// mod compare;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let framework = args.get(1).map(|s| s.as_str()).unwrap_or_else(|| {
        eprintln!("Usage: dhat_compare [fnrpc-web|xitca-web] [n]");
        std::process::exit(1);
    });
    let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    match framework {
        "fnrpc-web" | "fnrpc-web-macro" => rt.block_on(compare::fnrpc_web::bench_macro(n)),
        "fnrpc-web-manual" => rt.block_on(compare::fnrpc_web::bench_manual(n)),
        "fnrpc-web-post" => rt.block_on(compare::fnrpc_web::bench_post(n)),
        "fnrpc-web-f-text" => rt.block_on(compare::fnrpc_web_f::text::bench_text(n)),
        "fnrpc-web-f-text-owned" => {
            rt.block_on(compare::fnrpc_web_f::text::bench_text_raw_owned(n))
        }
        "fnrpc-web-f-text-raw" => rt.block_on(compare::fnrpc_web_f::text::bench_text_route_raw(n)),
        "fnrpc-web-f-text-empty" => rt.block_on(compare::fnrpc_web_f::text::bench_text_empty(n)),
        "fnrpc-web-f-raw-json" => rt.block_on(compare::fnrpc_web_f::raw_json::bench_null_json(n)),
        "fnrpc-web-subscribe" => rt.block_on(compare::fnrpc_web::bench_subscribe(n)),
        "fnrpc-web-sse" => rt.block_on(compare::fnrpc_web::bench_sse(n)),
        "fnrpc-web-mw" => rt.block_on(compare::fnrpc_web::bench_macro_mw(n)),
        "fnrpc-web-multi" => rt.block_on(compare::fnrpc_web::bench_macro_multi(n)),
        "fnrpc-xitca-web" => rt.block_on(compare::fnrpc_xitca_web::bench_macro(n)),
        "fnrpc-xitca-web-noop-raw" => rt.block_on(compare::fnrpc_xitca_web::bench_noop_raw(n)),
        "fnrpc-axum" => rt.block_on(compare::fnrpc_axum_web::bench_macro(n)),
        "fnrpc-axum-noop-raw" => rt.block_on(compare::fnrpc_axum_web::bench_noop_raw(n)),
        "axum" => rt.block_on(compare::axum_web::bench(n)),
        "xitca-web" => rt.block_on(compare::xitca_web::bench(n)),
        "xitca-web-null-json" => rt.block_on(compare::xitca_web::bench_null_json(n)),
        "xitca-web-f-text" => rt.block_on(compare::xitca_web_f::text::bench_text(n)),
        "xitca-web-f-raw-json" => rt.block_on(compare::xitca_web_f::raw_json::bench_null_json(n)),
        "xitca-web-mw" => rt.block_on(compare::xitca_web::bench_mw(n)),
        "xitca-web-multi" => rt.block_on(compare::xitca_web::bench_multi(n)),
        _ => {
            eprintln!("Unknown framework: {framework}");
            std::process::exit(1);
        }
    }
}
