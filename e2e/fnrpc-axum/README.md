# e2e: fnrpc-axum + TS client (packages/fnrpc-client)

A runnable end-to-end example proving the **client → server** BigInt path keeps
full precision and does **not** rely on a client-sent `meta` envelope — using
the *real* generated typed client (`createClient` + codegen `bindings.ts`),
including an SSE subscription.

- **Server** (`server/`): `fnrpc-axum` + `axum`.
  - `big_echo`, `big_echo_primitive`, `big_echo_primitive_post`,
    `big_echo_primitive_mutate`, `big_echo_mutate` accept `u64` / `i128` /
    `Vec<u64>` fields and return a string confirmation with the exact values
    decoded.
  - `tick_seq` is an SSE subscription (`#[fnrpc::rpc_subscribe]`) that emits a
    head message embedding the exact `start` value, then `count` tick messages.
  - `src/bin/gen_fnrpc.rs` regenerates the TS client bindings from the *same*
    router the server serves, so the types always match.
- **Client** (`client/`): uses `packages/fnrpc-client`'s `createClient` +
  `fetchTransport`, built from the generated `bindings.ts` (`Procedures` +
  `__procedureMeta`). `run.ts` regenerates bindings, spawns the Rust server,
  runs the typed assertions, and shuts down.

## Run

```bash
cd e2e/fnrpc-axum/client
bun run            # regenerates bindings, spawns the server, asserts, shuts down
```

Or run them separately:

```bash
# terminal 1 — start the server (and the codegen bin in another call)
cargo run --manifest-path e2e/fnrpc-axum/server/Cargo.toml
cargo run --bin gen_fnrpc --manifest-path e2e/fnrpc-axum/server/Cargo.toml

# terminal 2 — run the typed client assertions
cd e2e/fnrpc-axum/client
bun run run.ts
```

## What it proves

The TS client sends bigint fields as **JSON strings** (via `toRustJson`, no
`meta`). The server decodes them back to numbers using its own schema
(`fnrpc::serializer::decode_bigint_by_schema`). The asserted values include
`18446744073709551615` (u64 max) and `170141183460469231731687303715884105727`
(i128 max) — both far beyond JS's `2^53` safe-integer range, so if precision
were lost (e.g. by narrowing to a JS `number`) the assertion would fail.

It also exercises the typed `createClient` surface (query / mutate with GET and
POST) and the SSE `subscribe` transport: `tick_seq` confirms the `start` value
arrives at full precision and the streamed `n=0,n=1,n=2` ticks arrive in order.

## Note on the response direction

This example verifies the **request** path. The **response** path currently
sends bigint as bare JSON numbers, so a client would still lose precision when
*receiving* bigint values. Plumbing a `{ json, meta }` response envelope plus
client-side `deserialize` is separate, later work — which is why the server
returns a `String` confirmation here rather than echoing the bigint struct.
