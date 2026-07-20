import { createTanstackQueryUtils } from "@fnrpc/tanstack-query";
import { createClient, fetchTransport } from "@fnrpc/client";
import type { Procedures } from "./bindings";
import { __procedureMeta } from "./bindings";

export const fnrpc = createClient<Procedures>(
  fetchTransport({ url: "http://localhost:3000/fnrpc" }),
  __procedureMeta,
);

export const client = createTanstackQueryUtils(fnrpc);
