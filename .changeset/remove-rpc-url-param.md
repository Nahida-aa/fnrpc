---
"@fnrpc/client": minor
---

Remove the unused `rpc_url` parameter from `generate_ts_client` and `write_ts_client` (and `RpcRouter::generate_ts_client`). The base URL is a client-side runtime concern and was never embedded in the generated `bindings.ts`, so the parameter was dead. This is a breaking change to the codegen API.

Also adds a type-generation e2e test (`crates/fnrpc/tests/typegen.rs`) that locks down the Rust→TS mapping for scalars (bigint, `Option`/`Vec` of bigint, `f64` → `number | null`), nested structs, enums (unit + data variants), and the `RpcErr` error type.
