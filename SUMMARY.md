## Objective
- Redesign the fnrpc client API: direct-call `client.xxx(input)` without method suffixes, subscriptions as `AsyncIterable`, unified Rust HTTP routes with kind-based dispatch, runtime kind map

## Important Details
- No backward compat — old `.query()`/`.mutate()`/`.subscribe()` suffixes removed, `Observable`/`Unsubscribable`/`ExecuteArgs` types gone
- Naming: Rust `#[rpc_subscription]` → `#[rpc_subscribe]`, `#[rpc_mutation]` → `#[rpc_mutate]`; TS `ProcedureKind = "query" | "mutate" | "subscribe"`; Rust traits `RpcSubscription` → `RpcSubscribe`, `ErasedSubscriptionHandler` → `ErasedSubscribeHandler`
- Axum routes merged to single `GET+POST /fnrpc/{*path}`; handler calls `router.get_procedure_kind(path)` for dispatch
- Tauri command `rpc_subscribe` → `rpc_sub`
- `consumeEventIterator(iterable, opts)` for callback-style AsyncIterable consumption
- **BigInt fix**: cross-IPC (Tauri `JSON.stringify` can't handle BigInt). Solution: orpc-style `{ json, meta }` envelope on TS client side → Rust `unpack_meta()` unwraps before dispatch

## Work State
### Completed
- Rust side: macros (`rpc_subscribe`/`rpc_mutate`), handler traits, router (`get_procedure_kind`, `__procedureKinds` codegen), Axum merge, Tauri command rename — all compile + 26 tests pass (18 integration + 8 serializer)
- TS `types.ts`: `ProcedureKind = "query" | "mutate" | "subscribe"`; removed old types; added `ConsumeEventOptions`
- TS `createClient.ts`: `createClient<P>(transport, kindMap)` — direct proxy calls, no method suffixes
  - `ProcedureCallable` for subscribe: `(input, signal?) => Promise<AsyncIterable<output>>` (was `AsyncIterable<output>`)
  - `Transport` always returns `Promise<unknown>` (was `Promise<unknown> | AsyncIterable<unknown>`)
- TS `tauri.ts`: subscribe 返回 `Promise<AsyncIterable>` — invoke 成功后 resolve Promise 代表连接就绪
- TS `UntypedClient.ts`: fetchTransport subscribe 返回 `Promise<AsyncIterable>` — EventSource `onopen` 后 resolve Promise
  - `consumeEventIterator` 接受 `AsyncIterable<T> | Promise<AsyncIterable<T>>`（内部 await）
- TS `serializer.ts`: orpc-style `serialize(input)` → `{ json, meta }` with `BIGINT` type tag, `deserialize(serialized)` → restored value, `flattenForRust()` for Rust compat, `safeStringify()`
  - `walk()` 中 `undefined` → `null`，避免 Tauri IPC 丢 key
  - `flattenForRust()` / `deserialize()` 处理根级别 meta（segments 为空）
- Rust `serializer.rs`: `unpack_meta()` + `apply_root_fix()` — 处理根级别 BIGINT
- Tauri & Axum handlers: both call `unpack_meta()` before dispatch
- `fnrpc-tanstack-query`:
  - `ProcedureUtils` 新增 `streamedKey`、`streamedOptions`、`liveKey`、`liveOptions`
  - `stream-query.ts`: `serializableStreamedQuery` — 累积流到数组，支持 `refetchMode`/`maxChunks`
  - `live-query.ts`: `liveQuery` — 每个 chunk 通过 `setQueryData` 实时更新 cache，返回最后一个值
- Example client updated to `await fnrpc.echo_stream(prefix(), signal)` pattern

### Known
- Tick subscription in Tauri previously broken by `JSON.stringify(BigInt)` — now fixed via serializer envelope in transport layer + Rust-side unwrap
- `EventsUpdate` in the tick subscription has `b: BigInt` field — value traverses Tauri IPC via Channel as serialized string, reader parses as-is. No BigInt issues on output path since Channel sends strings, not JSON-serialized values.

---

# Architecture Reference

## Proc macros

| Macro | Kind | What it generates |
|---|---|---|
| `#[rpc_query]` | attr | renames fn to `{fn}_impl`, generates `struct {fn}` + `impl RpcFn<Ctx>` with `KIND = "query"` |
| `#[rpc_mutate]` | attr | same but `KIND = "mutate"` |
| `#[rpc_subscribe]` | attr | renames fn to `{fn}_impl`, generates `struct {fn}` + `impl RpcSubscribe<Ctx>` |

**Ctx inference**: first param `&T` → `Ctx = T`; otherwise `Ctx = ()` (generic impl over `T`).

**Multi-param**: Input is tuple-ized `(a: i32, b: String)` → dispatch via `input.0`, `input.1`.

**Subscribe is sync** (`fn`, not `async fn`) — returns `Pin<Box<dyn Stream + Send + '_>>` directly. Stream itself can contain async work.

## RpcRouter

```rust
RpcRouter::<Ctx>::new()
    .query(my_query)
    .mutate(my_mutate)
    .subscribe(my_subscribe)
    .layer(HookLayer::new()) // last added = outermost
    .layer(TracingLayer)
```

- `dispatch(ctx, path, input)` — builds RouterService, wraps in layers, calls through
- `get_sub_handler(path)` — returns `Arc<dyn ErasedSubscribeHandler>` for subscribe streams
- `codegen::generate_ts_client(router, url)` → TS bindings string
- `codegen::write_ts_client(router, url, path)` — write bindings to disk

## Handler traits

- **`RpcFn<Ctx>`** — typed: `Input: Type`, `Output: Type`. `exec(ctx, input) -> Result<Output, RpcErr>`
- **`ErasedHandler<Ctx>`** — object-safe: `call(ctx, Value) -> Result<Value, RpcErr>`. Blanket impl from `RpcFn`.
- **`RpcSubscribe<Ctx>`** — `exec(ctx, input) -> Pin<Box<dyn Stream<Item = Result<Output, RpcErr>> + Send + '_>>`
- **`ErasedSubscribeHandler<Ctx>`** — `call()` has `'a` lifetime for borrowing ctx.

## Middleware

- **`FnLayer<Ctx>`** — `layer(inner: Box<dyn FnService>) -> Box<dyn FnService>`
- **`HookLayer`** — `before(f)` / `after(f)` closures
- **`TracingLayer`** — `feature = "tracing"`, logs path/input/output/latency
- Layer ordering: last added = outermost (first to process, last to respond)

## Error handling (`RpcErr`)

```rust
pub struct RpcErr {
    pub name: String,      // always "RpcErr"
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}
```

Constructors: `internal(msg)`, `bad_request(msg)`, `not_found(msg)`, `new(code, msg)`, `.with_data(val)`.

`From<String>` / `From<&str>` → `code: "INTERNAL_SERVER_ERROR"`.

**specta rc.25 issue**: `serde_json::Value` does not implement `Type` in rc.25, so `RpcErr` cannot `#[derive(Type)]`. Manual `Type` impl in `error.rs` uses `NamedDataType::init_with_sentinel()`, maps `data` to `unknown | null` via `specta_typescript::define("unknown")`.

TS mirror: `packages/fnrpc-client/src/error.ts` — `class RpcError extends Error`, `name = "RpcErr"`, code/data fields.

## Transport layer

```typescript
type Transport = (path, input, kind, signal?) => Promise<unknown> | AsyncIterable<unknown>
```

| Transport | Query | Mutate | Subscribe |
|---|---|---|---|
| `fetchTransport` | `GET /{path}?input=` | `POST /{path}` body=JSON | `GET` via EventSource |
| `tauriTransport` | `invoke("rpc_fn")` | same | `invoke("rpc_sub")` + Channel<string> |

Both apply `serialize()` → `flattenForRust()` before sending.

## BigInt envelope

- Client `serializer.ts`: `{ json, meta }` envelope, BigInt → string + meta tag
- Server `serializer.rs`: `unpack_meta()` reconstructs
- `flattenForRust()`: BigInt strings → Numbers (lossy > 2^53, OK for serde_json)

## Key files

| File | Purpose |
|---|---|
| `crates/fnrpc/src/error.rs` | RpcErr struct + manual Type impl |
| `crates/fnrpc/src/handler.rs` | RpcFn, ErasedHandler, RpcSubscribe, ErasedSubscribeHandler |
| `crates/fnrpc/src/router.rs` | RpcRouter — dispatch, layers, TS generation |
| `crates/fnrpc/src/middleware.rs` | FnLayer, FnService, HookLayer, TracingLayer |
| `crates/fnrpc/src/serializer.rs` | Server-side BigInt unpacking |
| `crates/fnrpc-macros/src/lib.rs` | Proc macros |
| `packages/fnrpc-client/src/error.ts` | RpcError class |
| `packages/fnrpc-client/src/createClient.ts` | Typed Proxy-based client |
| `packages/fnrpc-client/src/UntypedClient.ts` | Fetch/SSE transport |
| `packages/fnrpc-client/src/tauri.ts` | Tauri IPC transport |
| `packages/fnrpc-client/src/serializer.ts` | BigInt envelope client-side |
| `packages/fnrpc-tanstack-query/src/` | TanStack Query integration utils |
| `examples/tauri-solid-tanstack/` | Full example: Tauri + Solid + TanStack Query |
| `examples/tauri-solid-tanstack/src-tauri/src/scripts/gen_fnrpc.rs` | Codegen script |

## Test index

| Test | What it tests |
|---|---|
| `test_manual_rpc` | Manual RpcFn impl, dispatch, unknown path error |
| `test_ctx_rpc` | Ctx-carrying manual handler |
| `test_macro_rpc` | `#[rpc_query]` macro dispatch |
| `test_macro_mutate_kind` | `kind() = "mutate"` |
| `test_macro_health_no_ctx` | No-ctx, no-param query |
| `test_macro_health_with_ctx` | Ctx = `()` query |
| `test_macro_ctx_rpc` | Ctx-carrying macro query |
| `test_subscribe` | Subscribe dispatch, stream collection |
| `test_subscribe_ctx` | Ctx-carrying subscribe |
| `test_subscribe_unknown_path` | Unknown subscribe path error |
| `test_multi_param` | Multi-param tuple input |
| `test_multi_param_ctx` | Multi-param with ctx |
| `test_multi_param_ts_info` | Tuple TS type `[number, number, string]` |
| `test_custom_layer` | Custom FnLayer |
| `test_hook_layer` | HookLayer before/after |
| `test_multiple_layers` | Layer ordering |
| `test_ts_client` | TS generation output |
| serializer unit tests (8) | BigInt unpack, envelope passthrough, null, nested |

## Patterns

- **Layers**: last added = outermost
- **Subscribe lifetimes**: macro generates `+ '_` for ctx borrowing
- **Error mapping**: macros map user `Err(e)` → `RpcErr::internal(e.to_string())`
- **Tuples in TS**: Rust tuples → `[type, type, ...]` arrays
- **u64/i64/usize/isize** → `bigint` (via semantic config `enable_lossless_bigints`)
- **f64** → `number | null` (JSON can't represent NaN/Infinity)
- **No TS tests exist yet**

## Known limits

- **specta rc.25**: `serde_json::Value` lacks `Type` → manual `init_with_sentinel` for `RpcErr`
- **BigInt precision**: `flattenForRust()` converts BigInt strings to Numbers (precision loss > 2^53)
- **No WASM/browser test suite** for client packages
- **Example excluded from workspace** — build/codegen must be done in example dir
