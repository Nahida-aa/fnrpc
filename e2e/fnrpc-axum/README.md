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
  - `camel_echo`, `flat_echo`, `wrapper_echo`, `payload_echo`, `newtype_echo`
    echo shapes whose **serde** wire form differs from their Rust form —
    `rename_all`, `#[serde(flatten)]`, a generic argument, an externally tagged
    enum, and a newtype struct. These are the shapes whose `meta` paths the
    server used to compute from the wrong view of the type.
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
(`fnrpc::serializer::decode_bigint_by_schema`).

The **response** direction is also covered end-to-end: the server encodes
bigint output as a `{ json, meta }` envelope via
`fnrpc::serializer::encode_bigint_by_schema` (driven by its own output schema),
and the TS client reconstructs `BigInt` values from the `meta` (via
`deserialize`). The asserted values include `18446744073709551615` (u64 max)
and `170141183460469231731687303715884105727` (i128 max) — both far beyond JS's
`2^53` safe-integer range, so if precision were lost (e.g. by narrowing to a JS
`number`) the assertion would fail. `big_echo` / `big_echo_mutate` / `big_out`
all return bigint structs, and `tick_seq` streams `u64` values over SSE.

It also exercises the typed `createClient` surface (query / mutate with GET and
POST) and the SSE `subscribe` transport.

The serde-shaped procedures additionally prove the `meta` paths name the keys
that actually exist on the wire: a `#[serde(rename_all)]` field is addressed by
its renamed key, a flattened field by its hoisted key, a generic argument by
where `T` was substituted, an enum payload under its variant key, and a newtype
without any wrapper segment. Mirrors
`crates/fnrpc/tests/bigint_wire.rs`, but over the real network and through the
real TS `deserialize`.
