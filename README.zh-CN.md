# fnrpc

**Rust + TypeScript 全栈类型安全 RPC 框架，原生支持 Axum 和 Tauri。**

Rust 中定义一次 RPC 函数，自动生成 TypeScript 类型。前端直接调用，全程类型安全——无需手动同步类型，无需在开发流程中嵌入代码生成步骤。

```rust
// Rust：一处定义
#[rpc_query]
async fn greet(ctx: &Ctx, name: String) -> String {
    format!("Hello {name}!")
}
```

```typescript
// TypeScript：全类型推导
const msg = await fnrpc.greet("world");
//    ^? string
```

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

### 2. 构建路由

```rust
use fnrpc::router::RpcRouter;

let router = RpcRouter::<()>::new()
    .query(greet)
    .query(add);
```

### 3. 接入 Axum

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
