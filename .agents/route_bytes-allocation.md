# route_bytes allocation analysis (dhat, per-request)

Verified with dhat call-path profiling (`benches/src/bin/dhat_callpath_text.rs`
+ `benches/src/bin/dhat_analyze.rs`). Source-level, not inference.

## Baseline (current, after c164c0f + 768464c)

`fnrpc-web` `route_bytes` handler returning `b"ok"`, `GET /text`, n=5000:

- **96 B / 2 blocks per op** (matches old AGENTS.md `noop_raw` record).
- Response body `b"ok"` is **zero-copy**: `Cow::Borrowed` → `Bytes::from_static`
  (no heap block). Confirmed absent from the call-path trace.

### The 2 blocks are BOTH `Box::pin` (future boxing)

dhat trace names them exactly:

1. `Box::pin ← <H as RawRpcFn>::exec` — `crates/fnrpc-macros/src/bytes_handler.rs:80`
   (the `#[fnrpc::rpc_bytes]` macro generates `exec` returning
   `Pin<Box<dyn Future>>`, so it boxes once).
2. `Box::pin ← BytesHandler::call` — `crates/fnrpc/src/router.rs:460`
   (`Handler::Bytes(Box<dyn BytesHandlerFn>)` is trait-object dispatched;
   `BytesHandlerFn::call` also returns `Pin<Box<dyn Future>>`, boxes again).

So `route_bytes` pays a **double box** because it goes through the
`Box<dyn BytesHandlerFn>` trait-object layer.

## Path String (the 3rd block, now removed)

Before c164c0f there was a 3rd block: `str::to_owned` in
`fnrpc_web::single_call` (`crates/fnrpc-web/src/lib.rs`, the `path.to_owned()`
introduced by commit `64bada39` for SSE/subscribe). It was a **lazy borrow
workaround**, not required by subscribe (all `dispatch`/`dispatch_subscribe`/
`has_subscribe` take `&str`). Fixed by borrowing `path` after `req.body_mut()`
(single_call) / reading body first (multi_call). Now `route_bytes` is back to
2 blks/op, and `/text` vs a 30-char path are identical (no `String` block
scaling with path length) — proving the `to_owned` is gone.

## Can the 2 Box::pin be merged into 1?

**Not by deleting a `Box::pin`.** Both are inherent to trait-object dispatch:
`RawRpcFn::exec` and `BytesHandlerFn::call` are trait methods whose return type
is fixed to `Pin<Box<dyn Future>>` (stable Rust cannot return `impl Future` from
a trait method without async-trait/RPITIT). The `Box::pin` lives in
`Handler::Bytes(Box<dyn BytesHandlerFn<Ctx>>)` (`crates/fnrpc/src/handler/mod.rs:314`).

To get to **1 block/op** you must change `route_bytes` from the
`Box<dyn BytesHandlerFn>` trait-object to **static dispatch** (like `route_fn`'s
`HandlerFn` uses generics). Caveat: `route_fn` (typed) goes through the
`Erased` slot and measures **6 blks/op** (heavier: `Extensions::new()` +
vtable `Box::pin`), so a naive "make route_bytes static like route_fn" would
make it *worse*. The win requires static dispatch **while keeping the Raw slot**
(zero `Box::pin` at `dispatch`, like `route_bytes` currently has at dispatch —
only the handler future boxes). See "attempt" below.

## Comparison table (dhat, n=5000)

| Path | slot | blocks/op | notes |
|------|------|-----------|-------|
| `route_bytes` `b"ok"` | Raw | **2** | 2× Box::pin (double box) |
| `route_bytes` long path | Raw | 2 | path String removed; no scaling |
| `route_fn` (typed JSON) | Erased | 6 | `Extensions` + vtable box + serde |
| `route_raw` `RpcOutput` | Erased | 7 | same Erased cost + RpcOutput wrap |

## Attempt: eliminate the 2nd Box::pin on the route_bytes path — DONE

**Simpler than expected.** The double box came from `BytesHandlerFn::call`
(`crates/fnrpc/src/router.rs`) wrapping `F::exec`'s already-boxed future in a
*second* `Box::pin(async move { F::exec(...).await })`. Since `RawRpcFn::exec`
(the `#[fnrpc::rpc_bytes]` macro) already returns `Pin<Box<dyn Future>>`,
`call` can just **forward `F::exec(ctx, input)` directly** — no second
`Box::pin`, no vtable box, no `?` (the `Result` is carried by the future).

This did NOT require changing the `Handler` enum to static dispatch: the
trait-object `Box<dyn BytesHandlerFn>` layer stays, but its `call` method now
transparently returns the inner boxed future. Only the macro's `Box::pin`
(`bytes_handler.rs:80`) remains.

### Result (dhat, n=5000, `b"ok"`)

- **48 B / 1 block per op** (down from 96 B / 2 blks).
- Trace confirms exactly one `Box::pin`, in `RawRpcFn::exec`; `BytesHandler::call`
  appears in the trace but allocates nothing.
- `/text` and a 30-char path are identical (48 B / 1 blk) — no path scaling.
- All transport tests pass (fnrpc 28+11, fnrpc-web 8, fnrpc-xitca 3,
  fnrpc-axum 5). Error propagation intact (the `?` was removed; `F::exec`'s
  `Result` is carried by the future).

### Why not do the same for `route_raw`?

`route_raw` registers into `HandlerSlot::Erased` (not Raw) and measures
7 blks/op; its cost is the `Erased` dispatch (`Extensions::new()` + vtable
`Box::pin`), not just a double box. A different, larger change would be needed
to bring it down. Out of scope for the route_bytes win.

### Caveat (middleware path)

When `route_bytes` is registered WITH middleware it goes through
`Box<dyn ErasedHandler>` (Erased slot, `router.rs` ~496), where
`ErasedHandler::call` (`router.rs:67`) still `Box::pin`s once on top of
`F::exec`. So the "1 block" win applies to the **no-middleware** Raw path;
the middleware path keeps its Erased overhead. That matches intent: Raw slot
is the zero-overhead tier.

## Tooling

- `cargo run -p benches --bin dhat_callpath_text --features dhat-heap -- 5000`
  writes `benches/target/dhat-text-{short,long}.json` (call-path traces for
  `/text` and a long path).
- `cargo run -p benches --bin dhat_analyze --features dhat-heap -- <json>`
  prints per-backtrace bytes/blocks, pinpointing each allocation's source line.
- `cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-f-text 5000`
  for the headline number.
