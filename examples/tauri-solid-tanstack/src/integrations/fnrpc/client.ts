import { createClient, fetchExecute, tauriExecute, ExecuteArgs } from "@fnrpc/client";
import type { Procedures } from "./bindings";
import { isTauri } from "@tauri-apps/api/core";
import { createSolidQueryHooks } from "@fnrpc/solid-query";
import { getQueryClient } from "#/integrations/tanstack-query/provider.ts";


export const client = createClient<Procedures>(
	isTauri() 
		? tauriExecute() 
		: (args) => fetchExecute({ url: "http://localhost:19110/fnrpc" }, args),
);

export const fnrpc = createSolidQueryHooks<Procedures>({
	client, queryClient: getQueryClient()
});
