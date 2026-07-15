import { createClient, fetchTransport, tauriTransport } from "@fnrpc/client";
import type { Procedures } from "./bindings";
import { __procedureKinds } from "./bindings";
import { createTanstackQueryUtils } from "@fnrpc/tanstack-query";
import { isTauri } from "@tauri-apps/api/core";

const transport = (() => {
  try {
    if (isTauri()) {
      return tauriTransport(() => import("@tauri-apps/api/core"));
    }
  } catch {
    // ignore
  }
  return fetchTransport({ url: "http://localhost:19110/fnrpc" });
})();

export const fnrpc = createClient<Procedures>(transport, __procedureKinds);

export const client = createTanstackQueryUtils(fnrpc);
