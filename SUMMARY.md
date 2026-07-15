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
- Rust side: macros (`rpc_subscribe`/`rpc_mutate`), handler traits, router (`get_procedure_kind`, `__procedureKinds` codegen), Axum merge, Tauri command rename — all compile + 25 tests pass (18 integration + 7 serializer)
- TS `types.ts`: `ProcedureKind = "query" | "mutate" | "subscribe"`; removed old types; added `ConsumeEventOptions`
- TS `UntypedClient.ts`: `fetchTransport(path, input, kind, signal?)` with SSE for subscribe, GET/POST for query/mutate; `consumeEventIterator`
- TS `createClient.ts`: `createClient<P>(transport, kindMap)` — direct proxy calls, no method suffixes
- TS `tauri.ts`: rewritten to accept `getCore: () => Promise<TauriCore>` and return transport function
- TS `serializer.ts`: orpc-style `serialize(input)` → `{ json, meta }` with `BIGINT` type tag, `deserialize(serialized)` → restored value, `flattenForRust()` for Rust compat, `safeStringify()`
- Rust `serializer.rs`: `unpack_meta()` — unwraps `{ json, meta }` envelope, applies BIGINT string→number conversion
- Tauri & Axum handlers: both call `unpack_meta()` before dispatch
- Example client, bindings, and routes updated to new API

### Known
- Tick subscription in Tauri previously broken by `JSON.stringify(BigInt)` — now fixed via serializer envelope in transport layer + Rust-side unwrap
- `EventsUpdate` in the tick subscription has `b: BigInt` field — value traverses Tauri IPC via Channel as serialized string, reader parses as-is. No BigInt issues on output path since Channel sends strings, not JSON-serialized values.
