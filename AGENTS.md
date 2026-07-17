# fnrpc

- specta pinned to `=2.0.0-rc.25` (manual `Type` impl for `RpcErr` — see `error.rs`)
- Workspace: `crates/*`, examples excluded
- Tests: `cargo test -p fnrpc` (26 tests)
- Regenerate bindings: `cd examples/tauri-solid-tanstack/src-tauri && cargo run --bin gen-fnrpc`
- Architecture, API, patterns → `SUMMARY.md`
- Benchmark guide → `.agents/benchmark-guide.md`
