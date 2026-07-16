# fnrpc

**Rust + TypeScript 全栈类型安全 RPC 框架，原生支持 Axum 和 Tauri。**

Rust 中定义一次 RPC 函数，自动生成 TypeScript 类型。前端直接调用，全程类型安全。

## 特性

- **单一数据源** — Rust 函数通过 specta 代码生成驱动 TypeScript 类型
- **双运行时** — 同一路由同时支持 Axum（HTTP）和 Tauri（IPC）
- **订阅** — Server-sent events（Axum）或 `Channel<string>`（Tauri），以 `AsyncIterable` 类型暴露
- **中间件** — HookLayer（前/后钩子）、TracingLayer（日志）、自定义 FnLayer
- **BigInt 安全** — 所有传输层自动处理 BigInt 序列化
- **TanStack Query** — `@fnrpc/tanstack-query` 提供 query/mutation/stream/live 工具函数

## 快速开始

### 1. 定义 RPC

```rust
use fnrpc::{rpc_query, rpc_mutate, rpc_subscribe, RpcErr};

// 单参数、无上下文
#[rpc_query]
async fn health_check() -> String {
    "ok".into()
}

// 多参数查询，带上下文
#[rpc_query]
async fn get_user(ctx: &Ctx, id: i64) -> Result<User, RpcErr> {
    // ctx.db.query(...)
    todo!()
}

// 变更 — 结构化入参
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

// 订阅 — 同步函数，返回 Stream
#[rpc_subscribe]
fn watch_user(ctx: &Ctx, id: i64) -> Pin<Box<dyn Stream<Item = Result<UserUpdate, RpcErr>> + Send + '_>> {
    // ...
}

// POST 订阅 — input 放在 body 而非 URL query params
#[rpc_subscribe("post")]
fn large_stream(ctx: &Ctx, input: LargeInput) -> Pin<Box<dyn Stream<Item = Result<Output, RpcErr>> + Send + '_>> {
    // ...
}
```

### 2. 构建路由

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

### 3. 接入 Axum

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

### 4. 生成 TypeScript 类型

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

### 5. 前端调用

```typescript
import { createClient, fetchTransport, tauriTransport } from "@fnrpc/client";
import type { Procedures } from "./bindings";
import { __procedureMeta } from "./bindings";
import { isTauri } from "@tauri-apps/api/core";

const transport = (() => {
  try {
    if (isTauri()) {
      return tauriTransport(() => import("@tauri-apps/api/core"));
    }
  } catch {}
  return fetchTransport({ url: "http://localhost:19110/fnrpc" });
})();

export const fnrpc = createClient<Procedures>(transport, __procedureMeta);
```

## 客户端调用

### 单参数

```typescript
const user = await fnrpc.get_user(42);
//        ^? User
```

### 多参数（元组入参）

Rust 多参数函数在 TypeScript 中接受一个元组：

```typescript
const add = await fnrpc.add([1, 2]); // fn add(a: i32, b: i32)
```

结构化入参直接传对象：

```typescript
const created = await fnrpc.create_user({ name: "Alice", email: "alice@example.com" });
```

### 订阅（AsyncIterable）

```typescript
const stream = await fnrpc.watch_user(42);
for await (const update of stream) {
    console.log("user updated:", update);
}
```

通过 `AbortSignal` 取消订阅：

```typescript
const controller = new AbortController();
const stream = await fnrpc.watch_user(42, controller.signal);

setTimeout(() => controller.abort(), 5000);
```

### 错误处理

```typescript
try {
    await fnrpc.get_user(999);
} catch (err) {
    if (isRpcError(err)) {
        // { code: "NOT_FOUND", message: "...", data: unknown }
    }
}
```

## TanStack Query 集成

使用 `@fnrpc/tanstack-query` 为你的 RPC 创建类型安全的 query/mutation/stream 工具。

### 初始化

```typescript
import { createTanstackQueryUtils } from "@fnrpc/tanstack-query";

export const client = createTanstackQueryUtils(fnrpc);
```

### React

```typescript
import { useQuery, useMutation } from "@tanstack/react-query";

// 查询
const { data: user } = useQuery(client.get_user.queryOptions(42));

// 变更
const mutation = useMutation(client.create_user.mutationOptions());
mutation.mutate({ name: "Alice", email: "alice@example.com" });

// Streamed — 累积流数据为数组
const { data: updates } = useQuery(client.watch_user.streamedOptions(42));

// Live — 每个 chunk 实时更新 cache
const { data: lastUpdate } = useQuery(client.watch_user.liveOptions(42));
```

### Solid

```typescript
import { createQuery, createMutation } from "@tanstack/solid-query";

// 查询
const query = createQuery(() => client.get_user.queryOptions(42));

// 变更
const mutation = createMutation(() => client.create_user.mutationOptions());
mutation.mutate({ name: "Alice", email: "alice@example.com" });

// Streamed
const streamed = createQuery(() => client.watch_user.streamedOptions(42));

// Live
const live = createQuery(() => client.watch_user.liveOptions(42));
```

## 包清单

| 包名 | 说明 |
|---------|-------------|
| `fnrpc` | Rust 核心：宏、路由、代码生成、中间件、Handler 特质 |
| `fnrpc-axum` | Axum 集成：`FnrpcState`、`handle` |
| `fnrpc-macros` | 过程宏：`#[rpc_query]`、`#[rpc_mutate]`、`#[rpc_subscribe]` |
| `@fnrpc/client` | TypeScript 类型安全客户端，基于 Proxy，支持 fetch 和 Tauri 传输层 |
| `@fnrpc/tanstack-query` | TanStack Query 工具函数：`queryOptions`、`mutationOptions`、`streamedQuery`、`liveQuery` |

## 示例

- [tauri-solid-tanstack](./examples/tauri-solid-tanstack) — Tauri + SolidJS + TanStack Query 全栈示例

## 架构

详细架构、过程宏参考、中间件文档、Handler 特质、测试索引见 [SUMMARY.md](./SUMMARY.md)。

## 路线图

见 [Roadmap.md](./Roadmap.md)。
