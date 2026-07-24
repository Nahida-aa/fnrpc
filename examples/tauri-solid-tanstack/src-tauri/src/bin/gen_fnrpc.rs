fn main() {
    let router = tauri_solid_tanstack_lib::integrations::fnrpc_func::build_fn_rpc_router();

    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_path = manifest_dir.join("../src/integrations/fnrpc/bindings.ts");

    fnrpc::gen_ts_client::write_ts_client(&router, &output_path)
        .expect("failed to write fnrpc client");

    println!("Generated {}", output_path.display());
}
