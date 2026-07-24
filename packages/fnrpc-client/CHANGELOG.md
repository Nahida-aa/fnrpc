# @fnrpc/client

## 0.4.0

### Minor Changes

- [`facc7b0`](https://github.com/Nahida-aa/fnrpc/commit/facc7b0ec02cfec92685a04c1e91bfdc5badf896) Thanks [@Nahida-aa](https://github.com/Nahida-aa)! - Remove the unused `rpc_url` parameter from `generate_ts_client` and `write_ts_client` (and `RpcRouter::generate_ts_client`). The base URL is a client-side runtime concern and was never embedded in the generated `bindings.ts`, so the parameter was dead. This is a breaking change to the codegen API.

  Also adds a type-generation e2e test (`crates/fnrpc/tests/typegen.rs`) that locks down the Rust→TS mapping for scalars (bigint, `Option`/`Vec` of bigint, `f64` → `number | null`), nested structs, enums (unit + data variants), and the `RpcErr` error type.

- [`1193862`](https://github.com/Nahida-aa/fnrpc/commit/1193862706c4abda2e1d8909a8c9a7867b6fb3a9) Thanks [@Nahida-aa](https://github.com/Nahida-aa)! - Upgrade specta to `2.0.0-rc.26` (tracked via `[patch.crates-io]` in the workspace `Cargo.toml`, the same git rev tauri-specta uses, since rc.26 is not yet published to crates.io).

  Codegen now applies serde attributes to the generated TypeScript through `specta-serde::PhasesFormat`, so `#[serde(rename = ...)]` — including **enum variant renames** — now appear in the generated `bindings.ts` (previously a no-op `Format` silently dropped them). BigInt-style Rust integers (u64/i64/u128/i128/usize/isize) are remapped to TS `bigint` via `specta-util::Remapper`, preserving lossless round-trips.

## 0.3.4

### Patch Changes

- [`3a9b651`](https://github.com/Nahida-aa/fnrpc/commit/3a9b651d862bcbd4bc2d82e72f19b2d691ae37e6) Thanks [@Nahida-aa](https://github.com/Nahida-aa)! - Fix UTF-8 corruption of non-ASCII (e.g. CJK) inputs sent over GET query strings.

  The server-side `percent_decode` (and the `fnrpc-axum` `urlencoding_decode` copy) cast each decoded byte directly to a `char`, which mangled multi-byte UTF-8 sequences into mojibake. Decoded bytes are now collected into a `Vec<u8>` and decoded as a single UTF-8 sequence via `from_utf8_lossy`.

  The `fnrpc-axum` e2e now covers `zh_input` (GET) and `zh_input_post` (POST) to assert end-to-end UTF-8 transparency.

## 0.3.3

### Patch Changes

- [`c6793c0`](https://github.com/Nahida-aa/fnrpc/commit/c6793c01e65eeee802ec297dd9dcc4fa4cd603df) Thanks [@Nahida-aa](https://github.com/Nahida-aa)! - Add BigInt-preserving wire format in both directions, fully backward compatible.

  - **Request (client → server):** the client sends bigint fields as JSON
    strings via `toRustJson` (no `meta` envelope). The server decodes them back
    to `u64`/`i64`/`i128`/... using its own specta schema
    (`decode_bigint_by_schema`), so no precision is lost above `2^53`.
  - **Response (server → client):** the server now encodes bigint output into a
    `{ json, meta }` envelope via `encode_bigint_by_schema` (schema-driven, no
    client negotiation). The TS client restores `BigInt` values through
    `deserialize`, including "\*" wildcard paths for lists/maps and SSE events.
    When the response has no bigint, plain JSON is returned as before.
  - Codegen (`gen_ts_client`) now emits the real `ProcedureMeta.method` instead
    of hardcoding `GET` for query/subscribe, so `#[rpc_query("post")]` and POST
    subscriptions get the correct HTTP method in `bindings.ts`/`__procedureMeta`.
  - Bundled a runnable e2e (`e2e/fnrpc-axum`) covering query/mutate (GET and
    POST) and SSE subscribe (GET and POST) with full BigInt precision.

## 0.3.2

### Patch Changes

- [`f299612`](https://github.com/Nahida-aa/fnrpc/commit/f299612c3403b97b0137206180dc9e3890bfc45d) Thanks [@Nahida-aa](https://github.com/Nahida-aa)! - Fix Tauri IPC command names to match the commands registered by `fnrpc-tauri`.

  `tauriTransport` was invoking `rpc_fn` / `rpc_sub` / `rpc_cancel_sub`, but
  `fnrpc-tauri`'s `generate_handler!` macro registers `__fnrpc_rpc_fn` /
  `__fnrpc_rpc_sub` / `__fnrpc_rpc_cancel_sub`. The unprefixed names never
  existed on the Rust side, so any Tauri-IPC call would fail with a
  "command not found" error. The transport now invokes the correct prefixed
  command names and passes the subscription cancellation id as `channel_id`.

  Also adds a unit test (`packages/fnrpc-client/test/tauri.test.ts`) asserting
  the invoked command names, and wires `bun run test` into CI so this class of
  regression is caught.

  Additionally fixes a pre-existing type error in `src/sse.ts` (the SSE
  `AsyncIterator` `next()` return type was incompatible with
  `IteratorResult<SSEEvent>`), and adds `@types/bun` plus a `tsconfig.test.json`
  so tests can `import { describe, it, expect } from "bun:test"` with full typing.

## 0.3.1

### Patch Changes

- [`0a90e08`](https://github.com/Nahida-aa/fnrpc/commit/0a90e08d9a7a18e0e2ec28ecd2535ca0dc26608d) Thanks [@Nahida-aa](https://github.com/Nahida-aa)! - Surface TypeScript codegen errors instead of silently dropping type definitions.

  Previously `gen_ts_client` used `.unwrap_or_default()` on the specta-typescript exporter result. When a type could not be exported (e.g. a BigInt-style Rust integer such as `u64`/`i64`/`usize` is forbidden by specta-typescript to avoid JS precision loss), the error was swallowed: `bindings.ts` was written missing type definitions while still emitting a dangling `Procedures` interface, and the generator exited successfully — making the failure invisible.

  The exporter error is now surfaced explicitly: codegen aborts with a clear message (the underlying specta error plus an fnrpc hint for fixing BigInt fields). Broken bindings are never produced silently.

  This affects the Rust `fnrpc` crate; the workspace version is bumped and all crates republished via `publish-rust.sh`.

## 0.3.0

### Minor Changes

- [`4cad97b`](https://github.com/Nahida-aa/fnrpc/commit/4cad97b78527291c8b876a372c2dbfab121074e9) Thanks [@Nahida-aa](https://github.com/Nahida-aa)! - feat: add SSE/subscribe endpoint support and fnrpc-tauri crate

  - All transport layers (fnrpc-web, fnrpc-axum, fnrpc-xitca) now detect
    subscribe handlers via `RpcRouter::has_subscribe()` and serve them as
    streaming SSE (`text/event-stream`) responses
  - Add `fnrpc-tauri` crate for Tauri IPC integration with subscription
    support and proper cancellation via `CancellationToken`
  - Fix subscribe input serialization: unpack BigInt meta envelope in
    transport layers before passing to `dispatch_subscribe`
  - Fix Tauri subscription lifecycle: `return()` now calls `rpc_cancel_sub`
    to stop the background task on client disconnect
  - Fix TanStack Query live/streamed queries: listen for abort signal and
    call `iterator.return()` to cancel the subscription on query cancel
  - Add `RpcRouter::has_subscribe()` method for path detection
  - Add `FnrpcState::from_arc()` for sharing routers across transports
  - Add dhat benchmarks for subscribe dispatch and SSE response
  - Add subscribe SSE integration tests for all transport layers
  - Update TypeScript client: `createSSEIterable` now properly cancels
    subscriptions via `rpc_cancel_sub` Tauri command

## 0.2.0

### Minor Changes

- [`2105499`](https://github.com/Nahida-aa/fnrpc/commit/2105499c48355727fd71c185cbab557cc0b1c16a) Thanks [@Nahida-aa](https://github.com/Nahida-aa)! - Subscribe POST method — `#[rpc_subscribe("post")]` sends input in request body; transport detects method from `__procedureMeta`

  Unified procedure metadata — `__procedureMeta` replaces `__procedureKinds`, includes `{ kind, method }` for each procedure

  fetch-based SSE transport — replaces EventSource, supports POST body and AbortSignal

  AbortSignal improvements — SSE reader cancels on signal abort, proper cleanup on iterator return

  Snake-case consistency — client method names match Rust function names

  New example: axum-react-query — full-stack example with React + TanStack Query
