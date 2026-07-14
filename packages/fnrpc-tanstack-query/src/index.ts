import type {
	Client,
	Procedure,
	Procedures,
	ProcedureProxyMethods,
} from "@fnrpc/client";
import { traverseClient, getQueryKey } from "@fnrpc/client";
import * as tanstack from "@tanstack/query-core";

export { skipToken } from "@tanstack/query-core";

// ── Call helpers ──────────────────────────────────────────

export function callQuery<P extends Procedures, K extends keyof P & string>(
	client: Client<P>,
	path: K,
	input: P[K]["input"],
): Promise<P[K]["output"]> {
	const segments = (path as string).split(".");
	const proxy = traverseClient(client, segments) as ProcedureProxyMethods<
		P[K] & Procedure
	>;
	return (proxy as any).query(input);
}

export function callMutation<P extends Procedures, K extends keyof P & string>(
	client: Client<P>,
	path: K,
	input: P[K]["input"],
): Promise<P[K]["output"]> {
	const segments = (path as string).split(".");
	const proxy = traverseClient(client, segments) as ProcedureProxyMethods<
		P[K] & Procedure
	>;
	return (proxy as any).mutate(input);
}

// ── Query options ─────────────────────────────────────────

export function createQueryOptions<
	P extends Procedures,
	K extends keyof P & string,
>(
	client: Client<P>,
	path: K,
	input: P[K]["input"],
): {
	queryKey: [K] | [K, P[K]["input"]];
	queryFn: () => Promise<P[K]["output"]>;
} {
	const segments = (path as string).split(".");
	const proxy = traverseClient(client, segments) as any;
	const key: [K] | [K, P[K]["input"]] = input === undefined ? [path] : [path, input];
	return {
		queryKey: key,
		queryFn: () => proxy.query(input),
	};
}

export function createMutationOptions<
	P extends Procedures,
	K extends keyof P & string,
>(
	client: Client<P>,
	path: K,
): {
	mutationKey: [string];
	mutationFn: (input: P[K]["input"]) => Promise<P[K]["output"]>;
} {
	return {
		mutationKey: [path as string],
		mutationFn: (input: P[K]["input"]) => callMutation(client, path, input),
	};
}

// ── Types ─────────────────────────────────────────────────

export type QueryKeyAndInput<P extends Procedures, K extends keyof P & string> = [
	key: K,
	...input: P[K]["input"] extends undefined | void | null
		? [undefined?]
		: [P[K]["input"]],
];

export interface Context<P extends Procedures> {
	client: Client<P>;
	queryClient: tanstack.QueryClient;
}

export interface BaseOptions<P extends Procedures> {
	fnrpc?: { client?: Client<P> };
}

export interface SubscriptionOptions<
	P extends Procedures,
	K extends keyof P & string,
> {
	onStarted?: () => void;
	onData: (data: P[K]["output"]) => void;
	onError?: (err: unknown) => void;
	onStopped?: () => void;
	onComplete?: () => void;
	enabled?: boolean;
	client?: Client<P>;
}

// ── Utils ─────────────────────────────────────────────────

export function createUtils<P extends Procedures>(
	client: Client<P>,
	queryClient: tanstack.QueryClient,
) {
	type K = keyof P & string;

	return {
		fetch: <T extends K>(path: T, input: P[T]["input"]) =>
			queryClient.fetchQuery({
				queryKey: getQueryKey(path as string, input),
				queryFn: () => callQuery(client, path, input),
			}),

		prefetch: <T extends K>(path: T, input: P[T]["input"]) =>
			queryClient.prefetchQuery({
				queryKey: getQueryKey(path as string, input),
				queryFn: () => callQuery(client, path, input),
			}),

		ensureData: <T extends K>(path: T, input: P[T]["input"]) =>
			queryClient.ensureQueryData({
				queryKey: getQueryKey(path as string, input),
				queryFn: () => callQuery(client, path, input),
			}),

		invalidate: <T extends K>(
			path: T,
			filters?: Omit<tanstack.InvalidateQueryFilters, "queryKey">,
			opts?: tanstack.InvalidateOptions,
		) => queryClient.invalidateQueries({ ...filters, queryKey: [path] }, opts),

		refetch: <T extends K>(
			path: T,
			filters?: Omit<tanstack.RefetchQueryFilters, "queryKey">,
			opts?: tanstack.RefetchOptions,
		) => queryClient.refetchQueries({ ...filters, queryKey: [path] }, opts),

		cancel: <T extends K>(
			path: T,
			filters?: Omit<tanstack.QueryFilters, "queryKey">,
			opts?: tanstack.CancelOptions,
		) => queryClient.cancelQueries({ ...filters, queryKey: [path] }, opts),

		setData: <T extends K>(
			path: T,
			input: P[T]["input"],
			updater: tanstack.Updater<
				P[T]["output"] | undefined,
				P[T]["output"] | undefined
			>,
			opts?: tanstack.SetDataOptions,
		) => {
			queryClient.setQueryData<P[T]["output"]>(
				getQueryKey(path as string, input),
				updater,
				opts,
			);
		},

		getData: <T extends K>(
			path: T,
			input: P[T]["input"],
		) =>
			queryClient.getQueryData<P[T]["output"]>(
				getQueryKey(path as string, input),
			),
	};
}
