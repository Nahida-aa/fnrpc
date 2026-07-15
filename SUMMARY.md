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
