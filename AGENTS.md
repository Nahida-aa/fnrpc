# fnrpc

- specta pinned to `=2.0.0-rc.25` (manual `Type` impl for `RpcErr` — see `error.rs`)
- Workspace: `crates/*`, examples excluded
- Tests:
  - `cargo test -p fnrpc` — 24 unit + integration tests
  - `cargo test -p fnrpc --features tracing` — +1 TracingLayer test
  - `cargo test -p fnrpc-web` — 6 integration tests (single/multi router)
  - `cargo test -p fnrpc-web --features file` — +2 static file tests
  - `cargo test -p fnrpc-xitca` — 2 integration tests
  - `cargo test -p fnrpc-axum` — 3 integration tests
- Regenerate bindings: `cd examples/tauri-solid-tanstack/src-tauri && cargo run --bin gen-fnrpc`
- Architecture, API, patterns → `SUMMARY.md` (may be outdated)
- Benchmark guide → `.agents/benchmark-guide.md`
- Roadmap → `ROADMAP.md`
- Allocation benchmarks: `cargo run -p benches --bin dhat_compare --features dhat-heap -- [name] [n]`

## Design philosophy

fnrpc is inspired by **Next.js Server Functions** and **TanStack Start Server Functions**, NOT tRPC.

Key differences from tRPC:
- No central router/procedure registry — each function is independently importable and callable
- No `.query()` / `.mutate()` distinction on the client — just `call("method", input)`
- The `RpcRouter` exists only as a server-side collection for HTTP transport; on the client, you just call functions
- `query`/`mutate` on the server side are just semantic hints (default HTTP method), not architectural boundaries

## Architecture

```
fnrpc (core)
├── middleware.rs        — RpcService, RpcLayer, PipelineT, combinators
├── middlewares/         — built-in middleware implementations
│   ├── hook.rs         —   HookLayer (before/after hooks)
│   └── tracing.rs      —   TracingLayer (structured logging)
├── router.rs           — RpcRouter, RpcRouterBuilder, InnerService
└── handler.rs          — RpcFn, RawRpcFn, Handler enum

fnrpc-web               — standalone HTTP server on xitca-http
├── App::new()          — single router, zero Box::pin
└── App::build()        — multi router (RPC + static files), one Box::pin

fnrpc-xitca             — integration with xitca-web
└── FnrpcState + handle — mount fnrpc into xitca-web App

fnrpc-axum              — integration with Axum
└── FnrpcState + handle — mount fnrpc into Axum Router
```

## Crate dependency

```
fnrpc-macros → fnrpc → fnrpc-web
                    → fnrpc-xitca
                    → fnrpc-axum
```

## Benchmark results (dhat, per 1000 ops)

| Benchmark | fnrpc-web | fnrpc-xitca-web | fnrpc-axum | xitca-web | axum |
|---|---|---|---|---|---|
| echo_get | **845B, 6blks** | 1,369B, 9blks | 2,496B, 22blks | 1,048B, 8blks | 2,373B, 20blks |
| echo_get + mw | **845B, 6blks** (zero) | — | — | 1,048B, 8blks | — |
| echo_get (multi + static) | **1,042B, 8blks** | — | — | 1,048B, 8blks | — |
| noop_raw | **96B, 2blks** | 624B, 5blks | — | 176B, 2blks | — |

- fnrpc-web single router: zero `Box::pin`, fastest path (845B vs xitca-web 1,048B, axum 2,373B)
- fnrpc-web multi router: one `Box::pin`, matches xitca-web (1,042B vs 1,048B)
- fnrpc-web noop_raw: **96B, 2blks** — matches the theoretical minimum (Handler::call directly)
- fnrpc-xitca-web: xitca-web framework tax (+321B vs fnrpc-web)
- fnrpc-axum: ≈ axum native (+123B, mostly Path+RawQuery extractors)
- **fnrpc-web is 2.8× more allocation-efficient than axum**
