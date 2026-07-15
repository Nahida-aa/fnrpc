fn main() {
    let router = tauri_solid_tanstack_lib::integrations::fnrpc_func::build_fn_rpc();

    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_path = manifest_dir.join("../src/integrations/fnrpc/bindings.ts");

    let rpc_url = "http://localhost:19110/fnrpc";
    fnrpc::codegen::write_ts_client(&router, rpc_url, &output_path)
        .expect("failed to write fnrpc client");

    println!("Generated {}", output_path.display());
}
