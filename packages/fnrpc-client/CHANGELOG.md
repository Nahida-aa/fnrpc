# @fnrpc/client

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
