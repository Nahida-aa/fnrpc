# route_bytes / dispatch allocation analysis (dhat, per-request)

Verified with dhat call-path profiling. Source-level, not inference.

## Tools

- `benches/src/bin/dhat_callpath_text.rs` — traces `route_bytes` (HTTP `App`)
  for short vs long path, to confirm no path-String block.
- `benches/src/bin/dhat_callpath_dispatch.rs` — **bypasses HTTP**, calls
  `RpcRouter::dispatch` directly with a Raw-slot handler (`route_bytes`) and an
  Erased-slot handler (`route_raw`), tracing each to its OWN JSON. This
  isolates `dispatch` internals (incl. `Extensions::new()`) from HTTP noise.
- `benches/src/bin/dhat_analyze.rs` — prints per-backtrace bytes/blocks,
  pinpointing each allocation's source line.

Usage:
```
cargo run -p benches --bin dhat_callpath_dispatch --features dhat-heap -- 5000
cargo run -p benches --bin dhat_analyze --features dhat-heap -- benches/target/dhat-dispatch-raw.json
cargo run -p benches --bin dhat_analyze --features dhat-heap -- benches/target/dhat-dispatch-erased.json
```

## Baseline (current, after c164c0f + 768464c + f0c27cd)

`fnrpc-web` `route_bytes` handler returning `b"ok"`, `GET /text`, n=5000:

- **48 B / 1 block per op** (single `Box::pin` in the macro-generated future).
- Response body `b"ok"` is **zero-copy**: `Cow::Borrowed` → `Bytes::from_static`
  (no heap block). Confirmed absent from the call-path trace.

### The 1 block is a single `Box::pin` (future boxing)

dhat trace (raw path, `dhat-dispatch-raw.json`) shows exactly one call chain:

```
Box::pin
← <H as RawRpcFn>::exec          — crates/fnrpc-macros/src/bytes_handler.rs:80
← <BytesHandler as BytesHandlerFn>::call   (forwards, allocates nothing)
← Handler::call
← RpcRouter::dispatch
```

The earlier "2 blocks / double box" was a real bug: `BytesHandlerFn::call`
wrapped `F::exec`'s already-boxed future in a second `Box::pin`. Fixed by
forwarding `F::exec` directly (see "Attempt" below). Only the macro's
`Box::pin` remains.

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
| `route_bytes` `b"ok"` | Raw | **1** (48 B) | single `Box::pin` in macro future |
| `route_bytes` long path | Raw | 1 | path String removed; no scaling |
| `route_raw` `RpcOutput` | Erased | **2** (96 B) | +1 vtable `Box::pin` vs Raw |
| `route_raw` + owned body | Erased | ~4 (100 B) | +1 `Vec` copy for `Cow::Owned` body |
| `route_fn` (typed JSON) | Erased | 6 | Erased + serde |
| `route_raw` + middleware | Erased | heavier | `Box<dyn ErasedHandler>` + Extensions |

## The route_raw vs route_bytes gap: MEASURED, not inferred

Both return `b"ok"` zero-copy (`Cow::Borrowed` → `Bytes::from_static`), so the
**response body contributes no block on either path**. The gap is purely the
**dispatch slot**:

- `route_bytes` → `HandlerSlot::Raw`: `dispatch` calls
  `handler.call(ctx, input, is_get)` directly — no `Extensions`, no vtable box
  beyond the macro future. 48 B / 1 blk.
- `route_raw` → `HandlerSlot::Erased`: `dispatch` (`router.rs:168-175`) does:
  ```rust
  let mut extensions = Extensions::new();
  handler.call(ctx, path, input, is_get, &mut extensions).await
  ```
  and `handler` is a `Box<dyn ErasedHandler>`, so the call crosses a
  **vtable `Box::pin` boundary** (`OutputHandler::call`, confirmed at the tail
  of backtrace chain 2 below). 96 B / 2 blks.

### dhat proof (dhat_callpath_dispatch, n=5000)

Erased trace (`dhat-dispatch-erased.json`) shows **two** call chains, each
240000 B / 5000 blks:

```
# chain 1 — handler-internal future boxing (same as Raw's single box)
Box::pin
← <H as RpcOutputFn>::exec
← <OutputHandler as RpcOutputHandlerFn>::call::{{closure}}
← Future::poll
← Handler::call
← RpcRouter::dispatch

# chain 2 — THE EXTRA BLOCK: vtable Box::pin at the Erased dispatch boundary
Box::pin
← <OutputHandler as RpcOutputHandlerFn>::call
← Handler::call
← RpcRouter::dispatch
```

Raw trace (`dhat-dispatch-raw.json`) shows **only chain-1-equivalent** (one
`Box::pin` in `RawRpcFn::exec`); there is no chain 2.

### CORRECTION: `Extensions::new()` itself does NOT allocate a heap block

`http::Extensions::new()` builds `Inner { map: None, .. }`; with `map: None`
it allocates **nothing** on the heap. The heap block is NOT `Extensions` — it
is the **vtable `Box::pin`** forced by going through `Box<dyn ErasedHandler>`
(backtrace chain 2). `Extensions::new()` is merely the *reason* the Erased slot
exists (middleware inter-op bus); the cost is the trait-object boundary, not
the empty bag.

So: the +48 B / +1 blk for `route_raw` vs `route_bytes` is the **Erased-slot
vtable `Box::pin`**, not `Extensions::new()`. The empty `Extensions` is still a
(present-but-heap-free) wart: it is constructed on every Erased dispatch even
when no middleware will ever insert into it.

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
- All transport tests pass. Error propagation intact (the `?` was removed;
  `F::exec`'s `Result` is carried by the future).

### Why not do the same for `route_raw`?

`route_raw` registers into `HandlerSlot::Erased` (not Raw) and measures
96 B / 2 blks/op. The extra block is the **Erased-slot vtable `Box::pin`**
(backtrace chain 2 above), not a double-box inside the handler. Bringing it to
1 blk would require `route_raw` to also register into the Raw slot when no
middleware is present — a larger change (the Erased signature carries
`path` + `&mut Extensions`, the Raw signature does not). Out of scope for the
route_bytes win; recorded here as a possible future optimization.

### Caveat (middleware path)

When `route_bytes` is registered WITH middleware it goes through
`Box<dyn ErasedHandler>` (Erased slot, `router.rs` ~496), where
`ErasedHandler::call` (`router.rs:67`) still `Box::pin`s once on top of
`F::exec`. So the "1 block" win applies to the **no-middleware** Raw path;
the middleware path keeps its Erased overhead. That matches intent: Raw slot
is the zero-overhead tier.

## Open question: make `Extensions::new()` lazy / conditional

`Extensions::new()` is heap-free when empty, so eliminating it is NOT an
allocation win today. It *would* matter if middleware are present (the
`Extensions` bag is the inter-op bus and will be filled). The real lever is the
**Erased-slot vtable `Box::pin`** (chain 2). Whether to:
- (a) construct `Extensions` only when middleware exist, or
- (b) route `route_raw` into the Raw slot when no middleware exist (kill chain 2)
is still under discussion — NOT yet implemented.
