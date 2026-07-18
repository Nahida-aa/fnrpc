# fnrpc

- specta pinned to `=2.0.0-rc.25` (manual `Type` impl for `RpcErr` — see `error.rs`)
- Workspace: `crates/*`, examples excluded
- Tests: `cargo test -p fnrpc` (26 tests)
- Regenerate bindings: `cd examples/tauri-solid-tanstack/src-tauri && cargo run --bin gen-fnrpc`
- Architecture, API, patterns → `SUMMARY.md`
- Benchmark guide → `.agents/benchmark-guide.md`

## Design philosophy

fnrpc is inspired by **Next.js Server Functions** and **TanStack Start Server Functions**, NOT tRPC.

Key differences from tRPC:
- No central router/procedure registry — each function is independently importable and callable
- No `.query()` / `.mutate()` distinction on the client — just `call("method", input)`
- The `RpcRouter` exists only as a server-side collection for HTTP transport; on the client, you just call functions
- `query`/`mutate` on the server side are just semantic hints (default HTTP method), not architectural boundaries
