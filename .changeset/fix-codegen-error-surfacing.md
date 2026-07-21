---
"@fnrpc/client": patch
---

Surface TypeScript codegen errors instead of silently dropping type definitions.

Previously `gen_ts_client` used `.unwrap_or_default()` on the specta-typescript exporter result. When a type could not be exported (e.g. a BigInt-style Rust integer such as `u64`/`i64`/`usize` is forbidden by specta-typescript to avoid JS precision loss), the error was swallowed: `bindings.ts` was written missing type definitions while still emitting a dangling `Procedures` interface, and the generator exited successfully — making the failure invisible.

The exporter error is now surfaced explicitly: codegen aborts with a clear message (the underlying specta error plus an fnrpc hint for fixing BigInt fields). Broken bindings are never produced silently.

This affects the Rust `fnrpc` crate; the workspace version is bumped and all crates republished via `publish-rust.sh`.
