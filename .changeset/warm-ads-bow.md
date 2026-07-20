---
"@fnrpc/client": minor
"@fnrpc/tanstack-query": minor
---

feat: add SSE/subscribe endpoint support and fnrpc-tauri crate

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
