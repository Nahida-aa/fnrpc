## Objective
- TypedHandler + at() registration (xitca-web–style): `#[rpc_query("get", "health")]` macro attrs, zero-sized marker structs with `TypedHandler` trait, `.at(handler)` on builder
- Rename `fnrpc::codegen` → `fnrpc::gen_ts_client`

## Important Details
- No backward compat — old `.query()`/`.mutate()`/`.subscribe()` suffixes removed, `Observable`/`Unsubscribable`/`ExecuteArgs` types gone
- Naming: Rust `#[rpc_subscription]` → `#[rpc_subscribe]`, `#[rpc_mutation]` → `#[rpc_mutate]`; TS `ProcedureKind = "query" | "mutate" | "subscribe"`; Rust traits `RpcSubscription` → `RpcSubscribe`, `ErasedSubscriptionHandler` → `ErasedSubscribeHandler`
- Axum routes merged to single `GET+POST /fnrpc/{*path}`; handler calls `router.get_procedure_kind(path)` for dispatch
- Tauri command `rpc_subscribe` → `rpc_sub`
- `consumeEventIterator(iterable, opts)` for callback-style AsyncIterable consumption
- **BigInt fix**: cross-IPC (Tauri `JSON.stringify` can't handle BigInt). Solution: orpc-style `{ json, meta }` envelope on TS client side → Rust `unpack_meta()` unwraps before dispatch

## Work State
### Completed (current session)
- `fnrpc::codegen` renamed to `fnrpc::gen_ts_client`; workspace refs updated (tests, examples, READMEs)
- Added `TypedHandler<Ctx>` / `TypedSubscribeHandler<Ctx>` traits in `handler.rs` — zero-sized marker pattern from xitca-web
- Proc macros (`rpc_query`/`rpc_mutate`/`rpc_subscribe`) now accept positional attribute args:
  - `#[rpc_query("get", "health")]` — method + path; both optional, default to fn name + "get"/"post"
  - Generated struct now implements `TypedHandler<Ctx>` in addition to `RpcFn<Ctx>`
- Added `.at<T: TypedHandler<Ctx>>(handler)` / `.at_sub<T: TypedSubscribeHandler<Ctx>>(handler)` on `RpcRouterBuilder`
- Old `.query()` / `.mutate()` / `.subscribe()` / `.route()` still work (backward compat)
- All 43 tests pass (fnrpc: 28, fnrpc-web: 5, fnrpc-xitca: 5, fnrpc-axum: 5)
### Completed (prior sessions)
- Rust side: macros (`rpc_subscribe`/`rpc_mutate`), handler traits, router (`get_procedure_kind`, `__procedureKinds` codegen), Axum merge, Tauri command rename — all compile + 26 tests pass (18 integration + 8 serializer)
- TS `types.ts`: `ProcedureKind = "query" | "mutate" | "subscribe"`; removed old types; added `ConsumeEventOptions`
- TS `createClient.ts`: `createClient<P>(transport, kindMap)` — direct proxy calls, no method suffixes
  - `ProcedureCallable` for subscribe: `(input, signal?) => Promise<AsyncIterable<output>>` (was `AsyncIterable<output>`)
  - `Transport` always returns `Promise<unknown>` (was `Promise<unknown> | AsyncIterable<unknown>`)
- TS `tauri.ts`: subscribe 返回 `Promise<AsyncIterable>` — invoke 成功后 resolve Promise 代表连接就绪
- TS `UntypedClient.ts`: fetchTransport subscribe 返回 `Promise<AsyncIterable>` — EventSource `onopen` 后 resolve Promise
  - `consumeEventIterator` 接受 `AsyncIterable<T> | Promise<AsyncIterable<T>>`（内部 await）
- TS `serializer.rs`: orpc-style `serialize(input)` → `{ json, meta }` with `BIGINT` type tag, `deserialize(serialized)` → restored value, `flattenForRust()` for Rust compat, `safeStringify()`
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
RpcRouterBuilder::<Ctx>::new()
    .query(my_query)
    .mutate(my_mutate)
    .subscribe(my_subscribe)
    .layer(HookLayer::new()) // last added = outermost
    .layer(TracingLayer)
    .build()
```

- `dispatch(ctx, path, input)` — calls through the monomorphized middleware chain (zero allocation per call inside the chain).
- `get_sub_handler(path)` — returns `Arc<dyn ErasedSubscribeHandler>` for subscribe streams
- `codegen::generate_ts_client(router, url)` → TS bindings string
- `codegen::write_ts_client(router, url, path)` — write bindings to disk

## Handler traits

- **`RpcFn<Ctx>`** — typed: `Input: Type`, `Output: Type`. `exec(ctx, input) -> Result<Output, RpcErr>`
- **`ErasedHandler<Ctx>`** — object-safe: `call(ctx, Value) -> Result<Value, RpcErr>`. Blanket impl from `RpcFn`.
- **`RpcSubscribe<Ctx>`** — `exec(ctx, input) -> Pin<Box<dyn Stream<Item = Result<Output, RpcErr>> + Send + '_>>`
- **`ErasedSubscribeHandler<Ctx>`** — `call()` has `'a` lifetime for borrowing ctx.

## Middleware

- **`FnLayer<Ctx, S>`** — `layer(inner: S) -> Self::Service` (generic over inner, associated output type)
- **`FnService<Ctx>`** — RPIT-based trait (zero `Box::pin` in monomorphized chain)
- **`ErasedFnService<Ctx>`** — dyn wrapper for storage in `Arc` (single `Box::pin` at boundary)
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

## Benchmark Data

In-process dhat allocation analysis: 20,000 requests per endpoint, single-threaded tokio runtime, release mode.

### Per‑request allocations (noop endpoint — minimum handler)

| Configuration | Bytes/op | Blocks/op | Δ vs. plain |
|---|---|---|---|
| **xitca‑web** (plain) | 177 | 3 | — |
| **fnrpc‑web** (bare xitca‑http) | 1 619 | 13 | — |
| **fnrpc‑xitca** | 2 414 | 17 | + 2 237 B, + 14 blks |
| **axum** (plain) | 1 032 | 13 | — |
| **fnrpc‑axum** | 3 451 | 29 | + 2 419 B, + 16 blks |
| **actix‑web** (plain) | 565 | 11 | — |
| **ntex** (plain) | 1 355 | 8 | — |

### Dispatch‑level allocations (no fnrpc‑web handler, only `dispatch_send`)

| Operation | Bytes/op | Blocks/op |
|---|---|---|
| `dispatch_send` (noop) | 201 | 3 |
| `dispatch_send` (echo String) | 242 | 5 |
| `dispatch_send` (not_found) | 165 | 3 |

### Allocation breakdown (per-backtrace)

The 3 dispatch‑level allocs come from three `Box::pin` boundaries:

| Size | Source | What |
|---|---|---|
| 128 B | `ErasedFnService::call` blanket | outer dispatch boundary |
| 72 B | `ErasedHandler::call` blanket | serde + exec wrapper |
| 1 B | async‑trait generated `Box::pin` | innermost handler future |

### Analysis

1. **fnrpc dispatch overhead is 3 allocs, ~200 B** — constant regardless of handler complexity.
2. **Framework overhead varies 3×–10×**: xitca‑web (3 blks) ≪ actix‑web (11) < axum (13) ≪ fnrpc‑axum (29). Bare xitca‑http (fnrpc‑web) is the most allocation‑efficient fnrpc host.
3. **fnrpc‑xitca adds 14 blks vs. plain xitca‑web**; of those, only 3 are dispatch_send — the rest is request/response parsing and the integration handler's extractors.
4. **fnrpc‑axum adds 16 blks vs. plain axum**; again only 3 come from dispatch_send.
5. **echo vs. noop** adds ~2 allocs (String serde) at the dispatch level; at the server level the difference is smaller because variable‑size allocations (e.g. serde_json buffers) subsume the extra bytes without new blocks.

### Next optimization targets

The 3 `Box::pin` allocations are the fnrpc core cost. To reduce 3→2:
- Merge `ErasedHandler` blanket `Box::pin` with the async‑trait `Box::pin` (the serde wrapper and the exec future share one allocation).

The integration‑layer allocations (10–13 per request) are harder to eliminate — they come from URI parsing, query‑string HashMap, body reading, and response serialization. These are framework‑agnostic costs of any HTTP RPC handler.

## Next Steps

1. **Radix tree router** — replace `Arc<RwLock<BTreeMap>>` with `xitca-router` (fork of `matchit`):
   - Build-time insertion (no per-request RwLock lock/unlock)
   - Zero-allocation path matching
   - Combined with `TypedHandler` / `.at()` for fully typed registration
2. **Path string elimination** — avoid `path.to_string()` in integration handlers by leveraging the radix tree's `at()` method
3. **Merge ErasedHandler Box::pin** — combine serde wrapper and exec future into one allocation
