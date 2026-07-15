# fnrpc

**Type-safe RPC for Rust + TypeScript, with Axum and Tauri support.**

Define your RPC functions once in Rust. TypeScript types are auto-generated. Call them from the frontend with full type safety.

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
use fnrpc::{rpc_query, rpc_mutate, rpc_subscribe, RpcErr};

// Single param, no context
#[rpc_query]
async fn health_check() -> String {
    "ok".into()
}

// Multi-param query with context
#[rpc_query]
async fn get_user(ctx: &Ctx, id: i64) -> Result<User, RpcErr> {
    // ctx.db.query(...)
    todo!()
}

// Mutation — structured input
#[derive(specta::Type, serde::Deserialize)]
struct CreateUserInput {
    name: String,
    email: String,
}

#[rpc_mutate]
async fn create_user(ctx: &Ctx, input: CreateUserInput) -> Result<User, RpcErr> {
    // INSERT INTO users (name, email) ...
    todo!()
}

// Subscription — sync fn returning a Stream
#[rpc_subscribe]
fn watch_user(ctx: &Ctx, id: i64) -> Pin<Box<dyn Stream<Item = Result<UserUpdate, RpcErr>> + Send + '_>> {
    // ...
}
```

### 2. Build router

```rust
use fnrpc::router::RpcRouter;

let router = RpcRouter::<Ctx>::new()
    .query(health_check)
    .query(get_user)
    .mutate(create_user)
    .subscribe(watch_user)
    .layer(HookLayer::new()
        .before(|ctx, path, input| tracing::info!("{path} invoked")))
    .layer(TracingLayer);
```

### 3. Serve with Axum

```rust
use std::sync::Arc;
use axum::Router;
use fnrpc_axum::{FnrpcState, handle};

Router::new()
    .route("/fnrpc/{*path}", axum::routing::get(handle::<Ctx>).post(handle::<Ctx>))
    .with_state(Arc::new(FnrpcState {
        router: Arc::new(router),
        ctx_from_headers: Arc::new(|headers| Ctx {
            db: db_pool.clone(),
            user_id: extract_user_id(&headers),
        }),
    }))
    .layer(cors);
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

## Client API

### Single param

```typescript
const user = await fnrpc.get_user(42);
//        ^? User
```

### Multi-param (tuple input)

Rust functions with multiple params accept a tuple in TypeScript:

```typescript
const user = await fnrpc.get_user([42, true]); // fn get_user(id: i64, include_deleted: bool)
```

For structured input, pass an object:

```typescript
const created = await fnrpc.create_user({ name: "Alice", email: "alice@example.com" });
```

### Subscription (AsyncIterable)

```typescript
const stream = await fnrpc.watch_user(42);
for await (const update of stream) {
    console.log("user updated:", update);
}
```

Abort a subscription via `AbortSignal`:

```typescript
const controller = new AbortController();
const stream = await fnrpc.watch_user(42, controller.signal);

setTimeout(() => controller.abort(), 5000);
```

### Error handling

```typescript
try {
    await fnrpc.get_user(999);
} catch (err) {
    if (isRpcError(err)) {
        // { code: "NOT_FOUND", message: "...", data: unknown }
    }
}
```

## TanStack Query integration

Use `@fnrpc/tanstack-query` to create typed query/mutation/stream utilities for your RPCs.

### Setup

```typescript
import { createTanstackQueryUtils } from "@fnrpc/tanstack-query";

export const client = createTanstackQueryUtils(fnrpc);
```

### React

```typescript
import { useQuery, useMutation } from "@tanstack/react-query";

// Query
const { data: user } = useQuery(client.getUser.queryOptions(42));

// Mutation
const mutation = useMutation(client.createUser.mutationOptions());
mutation.mutate({ name: "Alice", email: "alice@example.com" });

// Streamed — accumulates chunks into an array
const { data: updates } = useQuery(client.watchUser.streamedOptions(42));

// Live — each chunk updates the cache in real time
const { data: lastUpdate } = useQuery(client.watchUser.liveOptions(42));
```

### Solid

```typescript
import { createQuery, createMutation } from "@tanstack/solid-query";

// Query
const query = createQuery(() => client.getUser.queryOptions(42));

// Mutation
const mutation = createMutation(() => client.createUser.mutationOptions());
mutation.mutate({ name: "Alice", email: "alice@example.com" });

// Streamed
const streamed = createQuery(() => client.watchUser.streamedOptions(42));

// Live
const live = createQuery(() => client.watchUser.liveOptions(42));
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
