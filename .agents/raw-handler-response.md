# Raw handler response design (status code / headers)

Open design question for `route_bytes` (RawRpcFn). Captured so we can decide
how raw handlers evolve without breaking the framework-agnostic router.

## Current state (verified)

- `RawRpcFn::exec` returns `Result<Cow<'static, [u8]>, RpcErr>`
  (`crates/fnrpc/src/handler.rs`). Only a byte body + an error. **No status
  code, no response headers** in the handler's expressive range.
- `route_bytes` handlers are NOT registered into `router.procedures()`, so
  they never appear in `bindings.ts` / `__procedureMeta` and cannot be called
  via the typed `createClient`. (By design: raw handler bypasses serde + codegen.)
- Both transports assemble the HTTP response from `dispatch`'s
  `Result<(Cow<[u8]>, bool), RpcErr>`:
  - success → always `200 OK`; `content-type: application/json` only when
    `is_json` is true; no other headers.
  - error → `RpcErr.code` mapped to a fixed 400 / 404 / 500, body is
    `axum::Json(e)` / xitca json bytes.
- So "cannot express arbitrary status codes / custom response headers" is true
  for `route_bytes`, AND also for the typed `route_fn` path — the limitation
  lives in the transport layer, not in `route_bytes` specifically.

## Can a handler return the framework's `Response`?

### The underlying `http::Response` IS shared

`cargo tree -i http@1.4.2` shows axum, axum-core, xitca-http and every fnrpc
crate all resolve to the **same `http v1.4.2`**. So `axum::Response<T>` and
`xitca_web::WebResponse<B>` (`= Response<B>`, `xitca-web-0.8.1/src/lib.rs:380`)
are both aliases over the **same `http::Response` type**. Renaming does not
change type identity — the earlier claim that "they are not the same type"
was imprecise and is retracted.

### But the BODY type still differs

`http::Response<T>` is generic over its body. The two frameworks use different
body containers:

- axum: `http::Response<axum::body::Body>` (`Body` ≈ `http_body_util::Body`)
- xitca: `http::Response<xitca_web::body::ResponseBody>` (xitca's own type,
  built on `bytes::Bytes`)

So `Response<axum::Body>` ≠ `Response<xitca::ResponseBody>`. A handler that
returns one framework's `Response` cannot be consumed by the other transport,
and would force `fnrpc` core to depend on a concrete framework — breaking the
"one `RpcRouter` serves both axum and xitca" property.

### Conclusion

Returning a framework `Response` directly is NOT viable for the shared router.
The blocker is the body container divergence, not the `http::Response` header.

## Recommended direction (framework-agnostic response)

Introduce a framework-agnostic **output** type in `fnrpc` core, e.g.

```rust
pub struct RpcOutput {
    pub data: Cow<'static, [u8]>,          // the payload (neutral name, not "body")
    pub http: Option<HttpInfo>,            // HTTP-only extras; None for tauri/SSE/etc.
}

pub struct HttpInfo {
    pub status: Option<StatusCode>,        // None → transport default 200
    pub headers: Option<HeaderMap>,        // None → no extra headers
}
```

Named `RpcOutput` (not `RpcResponse`) so it stays neutral across transports:
tauri uses a command/channel model (no HTTP request/response), and "output" —
the result a call produces — reads naturally there and leaves room for future
transports. "Response" would wrongly imply an HTTP-only model. The payload is
`data` (not `body`) for the same reason; `status`/`headers` are HTTP-specific,
so they live in an optional `HttpInfo` that non-HTTP transports simply leave
as `None`.

- Handlers (typed or raw) may return `RpcOutput`; when `http` is `None` the
  transport uses its defaults (200, no extra headers), so a pure-payload path
  pays zero extra cost. `RpcErr` can be unified into this structure.
- Both transports convert `RpcOutput` → their own `Response<Body>` in one
  cheap step (one builder + body copy). Overhead is negligible; the real
  "native HTTP" performance of `route_bytes` comes from zero serde / zero
  envelope, which `RpcOutput { data: Cow<[u8]> }` preserves (zero-copy body).
- Extra cost (allocating `HttpInfo`, applying headers) is only paid when
  status/headers are actually needed.

### `route_bytes` evolution — DECIDED: option (1), two tiers

Implemented. `route_bytes` keeps the zero-copy `Cow<[u8]>` path (no status/headers),
and a new `route_raw` registers an `RpcOutputFn` handler that returns `RpcOutput`.

- `RpcOutput` lives in `crates/fnrpc/src/output.rs`; `RpcOutputFn` +
  `RpcOutputHandlerFn` in `crates/fnrpc/src/handler/rpc_output.rs`.
- `dispatch` (and the whole `RpcService`/`ErasedHandler`/`Handler` chain) now
  returns `RpcOutput` uniformly. Typed (`route_fn`) and bare-bytes (`route_bytes`)
  paths wrap their result as `RpcOutput { data, http: Some(json content-type) }`
  / `RpcOutput { data, http: None }` respectively. `route_raw` may set `http`.
- All three transports (axum / xitca / web) consume `RpcOutput`: default 200
  when `http` is `None`; otherwise apply `status` (if set) and merge `headers`
  (if set). Typed handlers keep `application/json` via the pre-set header.
- Convenience constructors: `RpcOutput::ok(data)`, `.with_status(code)`,
  `.with_header(name, value)` (both `&'static str`).

`route_bytes` is therefore NOT broken — it remains the lean zero-structure tier.
Option (2) (collapse `route_bytes` into `RpcOutput`) was rejected to avoid a
breaking change for existing raw handlers.

## Verification facts

- `http v1.4.2` is the single resolved version for both axum and xitca-http
  in this workspace (`Cargo.lock`).
- `RawRpcFn` return type: `Result<Cow<'static, [u8]>, RpcErr>` (unchanged).
- `RpcOutputFn` return type: `Result<RpcOutput, RpcErr>` (new, experimental).
- `route_bytes` is not pushed into `router.procedures()` (no codegen entry);
  `route_raw` is likewise not in codegen.
- axum body: `axum::body::Body`; xitca body: `xitca_web::body::ResponseBody`.

## Status

IMPLEMENTED (experimental) — two-tier model: `route_bytes` (`Cow<[u8]>`) and
`route_raw` (`RpcOutput`). `route_raw` is the experimental API for handlers
needing an HTTP status code / response headers. Follow `.agents/release.md`
for any version bump; do NOT hand-roll `cargo publish`.
