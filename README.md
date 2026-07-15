# fnrpc

**Type-safe RPC for Rust + TypeScript, with Axum and Tauri support.**

Define your RPC functions once in Rust. TypeScript types are auto-generated. Call them from the frontend with full type safety — no manual type sync, no code generation step in your workflow.

```rust
// Rust: define once
#[rpc_query]
async fn greet(ctx: &Ctx, name: String) -> String {
    format!("Hello {name}!")
}
```

```typescript
// TypeScript: fully typed
const msg = await fnrpc.greet("world");
//    ^? string
```

## Features

- **One source of truth** — Rust functions drive TypeScript types via specta codegen
- **Dual runtime** — Axum (HTTP) and Tauri (IPC) backends from the same router
- **Subscriptions** — Server-sent events (Axum) or `Channel<string>` (Tauri), typed as `AsyncIterable`
- **Middleware** — HookLayer before/after, TracingLayer, custom FnLayer
- **BigInt safe** — Automatic BigInt envelope across all transports
- **TanStack Query** — `@fnrpc/tanstack-query` for query/mutation/stream/live utilities

## Quick start

### 1. Define RPCs

```rust
use fnrpc::rpc_query;

#[rpc_query]
async fn greet(name: String) -> String {
    format!("Hello {name}!")
}

#[rpc_query]
async fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

### 2. Build router

```rust
use fnrpc::router::RpcRouter;

let router = RpcRouter::<()>::new()
    .query(greet)
    .query(add);
```

### 3. Serve with Axum

```rust
use std::sync::Arc;
use axum::Router;
use fnrpc_axum::{FnrpcState, handle};

Router::new()
    .route("/fnrpc/{*path}", axum::routing::get(handle::<()>).post(handle::<()>))
    .with_state(Arc::new(FnrpcState {
        router: Arc::new(router),
        ctx_from_headers: Arc::new(|_headers| ()),
    }));
```

### 4. Generate TypeScript types

```rust
// scripts/gen_fnrpc.rs
fn main() {
    let router = build_router();
    fnrpc::codegen::write_ts_client(
        &router,
        "http://localhost:3000/fnrpc",
        Path::new("../src/bindings.ts"),
    )
    .expect("codegen failed");
}
```

### 5. Call from TypeScript

```typescript
import { createClient, fetchTransport, tauriTransport } from "@fnrpc/client";
import type { Procedures } from "./bindings";
import { __procedureKinds } from "./bindings";
import { isTauri } from "@tauri-apps/api/core";

const transport = (() => {
  try {
    if (isTauri()) {
      return tauriTransport(() => import("@tauri-apps/api/core"));
    }
  } catch {}
  return fetchTransport({ url: "http://localhost:19110/fnrpc" });
})();

export const fnrpc = createClient<Procedures>(transport, __procedureKinds);
```

## Packages

| Package | Description |
|---------|-------------|
| `fnrpc` | Rust core: macros, router, codegen, middleware, handler traits |
| `fnrpc-axum` | Axum integration: `FnrpcState`, `handle` |
| `fnrpc-macros` | Proc macros: `#[rpc_query]`, `#[rpc_mutate]`, `#[rpc_subscribe]` |
| `@fnrpc/client` | TypeScript typed client, Proxy-based, fetch & Tauri transports |
| `@fnrpc/tanstack-query` | TanStack Query utilities: `queryOptions`, `mutationOptions`, `streamedQuery`, `liveQuery` |

## Examples

- [tauri-solid-tanstack](./examples/tauri-solid-tanstack) — Tauri + SolidJS + TanStack Query

## Architecture

See [SUMMARY.md](./SUMMARY.md) for detailed architecture, proc macro reference, middleware docs, handler traits, and test index.

## Roadmap

See [Roadmap.md](./Roadmap.md).
