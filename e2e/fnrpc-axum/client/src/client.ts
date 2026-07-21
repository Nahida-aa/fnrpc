/**
 * Typed fnrpc client for the e2e example.
 *
 * `Procedures` and `__procedureMeta` are generated from the server's router by
 * `cargo run --bin gen_fnrpc` (see `server/src/bin/gen_fnrpc.rs`).
 */
import { createClient, fetchTransport } from "../../../../packages/fnrpc-client/src";
import type { Procedures } from "./bindings";
import { __procedureMeta } from "./bindings";

export const fnrpc = createClient<Procedures>(
  fetchTransport({ url: "http://localhost:3000" }),
  __procedureMeta,
);
