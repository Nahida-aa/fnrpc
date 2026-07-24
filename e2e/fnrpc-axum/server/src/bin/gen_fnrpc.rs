//! Codegen binary: builds the e2e router and writes the TypeScript client
//! bindings (`bindings.ts`) consumed by `e2e/fnrpc-axum/client`.
//!
//! Run:
//!   cargo run --bin gen_fnrpc --manifest-path e2e/fnrpc-axum/server/Cargo.toml
//!
//! The output path is resolved relative to this crate's manifest directory so
//! the command works regardless of the current working directory.

use std::path::PathBuf;

fn main() {
    let router = e2e_fnrpc_axum_server::build_fn_rpc_router();

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let output_path: PathBuf = PathBuf::from(manifest_dir)
        .join("..")
        .join("client")
        .join("src")
        .join("bindings.ts");

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).expect("failed to create client/src dir");
    }

    fnrpc::gen_ts_client::write_ts_client(&router, &output_path)
        .expect("failed to write TS bindings");

    println!("wrote bindings -> {}", output_path.display());
}
