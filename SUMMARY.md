# fnrpc Architecture Reference

> This document describes the current architecture after the zero-erasure middleware refactor.
> Last updated: 2026-07-20

## Core crate (`fnrpc`)

### Modules

| Module | Contents |
|---|---|
| `middleware.rs` | `RpcService`, `RpcLayer`, `PipelineT`, `AsyncFnMiddleware`, `ServiceExt`, `NextExt`, `ClosureService` |
| `middlewares/` | Built-in middleware: `hook::HookLayer`, `tracing::TracingLayer` (feature = "tracing") |
| `router.rs` | `RpcRouter<Ctx>`, `RpcRouterBuilder<Ctx>`, `ErasedHandler<Ctx>`, `HandlerSlot<Ctx>` |
| `handler.rs` | `RpcFn<Ctx>`, `RpcFnExt<Ctx>`, `RawRpcFn<Ctx>`, `RpcSubscribe<Ctx>`, `SubscribeExt<Ctx>`, `ErasedSubscribeHandler<Ctx>`, `Handler<Ctx>`, `HandlerFn<Ctx>`, `BytesHandlerFn<Ctx>` |
| `error.rs` | `RpcErr` |
| `codec.rs` | `JsonCodec` |
| `serializer.rs` | BigInt unpacking |
| `gen_ts_client.rs` | TypeScript codegen |

### Middleware system

Key trait:
```rust
pub trait RpcService<Ctx> {
    type Response;  // = (Cow<'static, [u8]>, bool)
    type Error;     // = RpcErr
    fn call<'a>(&'a self, ctx: &'a Ctx, path: &'a str, input: &'a [u8],
                is_get: bool, extensions: &'a mut Extensions)
        -> impl Future<Output = Result<Self::Response, Self::Error>> + 'a;
}
```

- RPIT-based — no `#[async_trait]`, no hidden `Box::pin` in monomorphized chain
- `&[u8]` instead of `Value` — zero serde overhead in middleware chain
- Generic over `S` — entire middleware chain monomorphized at compile time

Layer ordering: **LIFO** — last `.layer()` added = outermost.

### RpcRouter

```rust
RpcRouterBuilder::<Ctx>::new()
    .route_fn(my_query)          // typed handler (RpcFn)
    .route_bytes(my_raw)         // raw bytes handler
    .layer(HookLayer::new()...)  // middleware (applied to handlers registered AFTER this)
    .build()                     // returns RpcRouter<Ctx>
```

- **Two-phase routing**: middleware is applied to each handler at `route_fn` time (not wrapped around the whole router). This matches xitca's approach.
- **Zero `Box::pin` without middleware**: handlers are stored as `Arc<Handler<Ctx>>` directly, called via `Handler::call` with no indirection.
- **One `Box::pin` with middleware**: handlers are type-erased to `Box<dyn ErasedHandler>`, called via vtable dispatch — one `Box::pin` at the dispatch boundary.
- **LIFO layer order**: last `.layer()` added = outermost middleware.
- **Layer order matters**: add layers before registering handlers. Layers only affect handlers registered after them.
- `router.dispatch(ctx, path, input, is_get)` — calls through middleware chain
- `router.dispatch_subscribe(ctx, path, input)` — returns a stream for subscribe handlers
- `router.procedures()` — metadata for TS codegen
- `router.generate_ts_client(url)` — generate TS client code

Builder methods:
- `builder.route_fn(handler)` — register typed RPC function
- `builder.route_bytes(handler)` — register raw bytes handler
- `builder.subscribe(handler)` — register subscribe handler
- `builder.layer(layer)` — add middleware layer
- `builder.layer_fn(func)` — add closure-based middleware

### Proc macros

| Macro | Kind | Generated struct implements |
|---|---|---|
| `#[rpc_query]` | query (GET) | `RpcFn<Ctx>` with `KIND = "query"`, `METHOD = "GET"` |
| `#[rpc_mutate]` | mutate (POST) | `RpcFn<Ctx>` with `KIND = "mutate"`, `METHOD = "POST"` |
| `#[rpc_subscribe]` | subscribe | `RpcSubscribe<Ctx>` |
| `#[rpc_bytes]` | raw bytes | `RawRpcFn<Ctx>` |

Ctx inference: first param `&T` → `Ctx = T`; otherwise `Ctx = ()`.

## HTTP transport crates

### `fnrpc-web` — standalone server on xitca-http

Two modes:

1. **Single router** (`App::new(router, ctx_factory)`) — zero `Box::pin`
   - `App::call(req)` → direct `router.dispatch()`
   - `App::run(addr)` — bind HTTP server

2. **Multi router** (`App::build(ctx_factory).rpc(...).static_dir(...).run(addr)`)
   - `AppBuilder<Ctx>` — add RPC routes and static dirs
   - Radix tree routing via `xitca_router::Router`
   - One `Box::pin` at route boundary
   - Static file serving with `feature = "file"`

### `fnrpc-xitca` — integration with xitca-web

```rust
use fnrpc_xitca::{FnrpcState, handle};

let state = FnrpcState::new(router, |_| ());
App::new()
    .with_state(state)
    .at("/{*path}", get(fn_service(handle::<MyCtx>)))
    .serve()...
```

- `FnrpcState<Ctx>` — holds router + ctx_factory
- `handle::<Ctx>` — async fn that extracts request from `WebContext`, calls `router.dispatch()`

### `fnrpc-axum` — integration with Axum

```rust
use fnrpc_axum::{FnrpcState, handle};

let state = Arc::new(FnrpcState::new(router, |_| ()));
let app = Router::new()
    .route("/{*path}", get(handle::<MyCtx>).post(handle::<MyCtx>))
    .with_state(state);
```

- `FnrpcState<Ctx>` — holds router + ctx_factory
- `handle::<Ctx>` — async fn, Axum extractor-based, calls `router.dispatch()`

## Benchmark results

See `AGENTS.md` or run:
```bash
# All available scenarios
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-macro 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-mw 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-multi 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-noop-raw 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-subscribe 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-sse 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-xitca-web 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-axum 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- xitca-web 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- axum 1000
```

## Test index

| Command | Tests |
|---|---|
| `cargo test -p fnrpc` | 23 tests (core + middleware + subscribe) |
| `cargo test -p fnrpc --features tracing` | +1 TracingLayer test |
| `cargo test -p fnrpc-web` | 6 tests (single/multi router) |
| `cargo test -p fnrpc-web --features file` | +2 static file tests |
| `cargo test -p fnrpc-xitca` | 2 tests |
| `cargo test -p fnrpc-axum` | 3 tests |
