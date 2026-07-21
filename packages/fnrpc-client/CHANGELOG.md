# @fnrpc/client

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
