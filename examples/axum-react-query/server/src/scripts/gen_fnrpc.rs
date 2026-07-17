use std::path::Path;

fn main() {
    let router = axum_react_query_server::rpc_func::build_fn_rpc_router();

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_path = manifest_dir.join("../src/integrations/fnrpc/bindings.ts");

    let rpc_url = "http://localhost:3000/fnrpc";
    fnrpc::gen_ts_client::write_ts_client(&router, rpc_url, &output_path)
        .expect("failed to write fnrpc client");

    println!("Generated {}", output_path.display());
}
