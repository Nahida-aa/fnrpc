import { createClient, fetchExecute, tauriExecute, ExecuteArgs } from "@fnrpc/client";
import type { Procedures } from "./bindings";
import { isTauri } from "@tauri-apps/api/core";
// import { createSolidQueryHooks } from "@fnrpc/solid-query";
import { getQueryClient } from "#/integrations/tanstack-query/provider.ts";
import { createTanstackQueryUtils } from "@fnrpc/tanstack-query";

/**
 * ```ts
 * fnrpc.health_check.query()
 * ```
 * 
 * tanstack query (不绑定框架)
 * ```ts
 * client.
 */
export const fnrpc = createClient<Procedures>(
	isTauri() 
		? tauriExecute() 
		: (args) => fetchExecute({ url: "http://localhost:19110/fnrpc" }, args),
);

export const client = createTanstackQueryUtils(fnrpc)

// export const fnrpcHook = createSolidQueryHooks<Procedures>({
// 	client: fnrpc, queryClient: getQueryClient()
// });
