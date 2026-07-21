---
"@fnrpc/client": patch
---

Add BigInt-preserving wire format in both directions, fully backward compatible.

- **Request (client → server):** the client sends bigint fields as JSON
  strings via `toRustJson` (no `meta` envelope). The server decodes them back
  to `u64`/`i64`/`i128`/... using its own specta schema
  (`decode_bigint_by_schema`), so no precision is lost above `2^53`.
- **Response (server → client):** the server now encodes bigint output into a
  `{ json, meta }` envelope via `encode_bigint_by_schema` (schema-driven, no
  client negotiation). The TS client restores `BigInt` values through
  `deserialize`, including "*" wildcard paths for lists/maps and SSE events.
  When the response has no bigint, plain JSON is returned as before.
- Codegen (`gen_ts_client`) now emits the real `ProcedureMeta.method` instead
  of hardcoding `GET` for query/subscribe, so `#[rpc_query("post")]` and POST
  subscriptions get the correct HTTP method in `bindings.ts`/`__procedureMeta`.
- Bundled a runnable e2e (`e2e/fnrpc-axum`) covering query/mutate (GET and
  POST) and SSE subscribe (GET and POST) with full BigInt precision.
