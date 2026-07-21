---
"@fnrpc/client": patch
---

Fix Tauri IPC command names to match the commands registered by `fnrpc-tauri`.

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
