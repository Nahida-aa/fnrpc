import type {
	Client,
	Procedure,
	Procedures,
	ProcedureProxyMethods,
} from "@fnrpc/client";
import { traverseClient, getQueryKey } from "@fnrpc/client";
import * as tanstack from "@tanstack/query-core";

// ── Helpers ──────────────────────────────────────────────

function callQuery<P extends Procedures, K extends keyof P & string>(
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

function callMutation<P extends Procedures, K extends keyof P & string>(
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

export type QueryKeyAndInput<P extends Procedures, K extends keyof P & string> = [
	key: K,
	...input: P[K]["input"] extends undefined | void | null
		? [undefined?]
		: [P[K]["input"]],
];

export type QueryKeyAndInputOrSkip<P extends Procedures, K extends keyof P & string> =
	| QueryKeyAndInput<P, K>
	| [K, tanstack.SkipToken];

function isSkipTokenInput(
	array: unknown[],
): array is [tanstack.SkipToken] {
	return array.length === 1 && array[0] === tanstack.skipToken;
}

// ── Context ──────────────────────────────────────────────

export interface Context<P extends Procedures> {
	client: Client<P>;
	queryClient: tanstack.QueryClient;
}

// ── Base options ─────────────────────────────────────────

export interface BaseOptions<P extends Procedures> {
	fnrpc?: { client?: Client<P> };
}

// ── Wrapped options (for hook helpers) ───────────────────

export type WrapQueryOptions<P extends Procedures, T> = Omit<
	T,
	"queryKey" | "queryFn"
> &
	BaseOptions<P>;

export type WrapMutationOptions<P extends Procedures, T> = Omit<
	T,
	"_defaulted" | "variables" | "mutationKey"
> &
	BaseOptions<P>;

// ── Subscription options ─────────────────────────────────

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

// ── createUtils ──────────────────────────────────────────

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
			updater: tanstack.Updater<P[T]["output"] | undefined, P[T]["output"] | undefined>,
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
		) => queryClient.getQueryData<P[T]["output"]>(
			getQueryKey(path as string, input),
		),
	};
}

// ── createHookHelpers ────────────────────────────────────

export function createHookHelpers<P extends Procedures>(args: {
	useContext(): Context<P> | null;
}) {
	type K = keyof P & string;
	type TBaseOptions = BaseOptions<P>;
	function useContext() {
		console.log('createHookHelpers args.useContext() before')
		const ctx = args.useContext();
		if (!ctx) throw new Error("fnrpc context provider not found!");
		console.log('createHookHelpers args.useContext() after')
		return ctx;
	}
	function useClient() {
		console.log('createHookHelpers useClient before')
		return useContext().client;
	}

	function getClient<T extends TBaseOptions>(opts?: T) {
		return (opts)?.fnrpc?.client ?? useClient();
	}

	function useQueryArgs<
		T extends K,
		O extends WrapQueryOptions<
			P, 
			tanstack.QueryObserverOptions<
				P[T]["output"],
				Error,
				P[T]["output"],
				P[T]["output"],
				QueryKeyAndInput<P, T>
			>
		>,
	>(keyAndInput: QueryKeyAndInput<P, T> | [T, tanstack.SkipToken], opts?: O) {
		console.log('useQueryArgs client = getClient(opts) before')
		const client = getClient(opts);
		console.log('useQueryArgs client = getClient(opts)')
		const [key, ...rest] = keyAndInput;
		const input = rest[0] as P[T]["input"] | undefined;

		return {
			...opts,
			queryKey: keyAndInput,
			queryFn: isSkipTokenInput(keyAndInput)
				? (tanstack.skipToken)
				: () => callQuery(client, key, input as P[T]["input"]),
		};
	}

	function useMutationArgs<
		T extends K,
		O extends WrapMutationOptions<
			P, tanstack.MutationObserverOptions<
				P[T]["output"],
				Error,
				P[T]["input"],
				unknown
			>
		>,
	>(key: T, opts?: O) {
		const client = getClient(opts);

		return {
			...opts,
			mutationKey: [key],
			mutationFn: (input: P[T]["input"]) =>
				callMutation(client, key, input),
		};
	}

	function handleSubscription<T extends K>(
		keyAndInput: QueryKeyAndInput<P, T> | [T, tanstack.SkipToken],
		opts: () => SubscriptionOptions<P, T>,
		_client: Client<P>,
	) {
		const options = opts();
		const [key, ...rest] = keyAndInput;
		const input = rest[0] as P[T]["input"] | undefined;

		if (!(options.enabled ?? true) || isSkipTokenInput(keyAndInput)) return;

		const client = options.client ?? _client;
		let isStopped = false;

		const segments = (key as string).split(".");
		const proxy = traverseClient(client, segments) as any;

		proxy
			.subscribe(input)
			?.then?.((unsub: () => void) => {
				if (isStopped) {
					unsub();
					return;
				}
				options.onStarted?.();
			})
			.catch((err: unknown) => {
				if (!isStopped) options.onError?.(err);
			});

		return () => {
			isStopped = true;
		};
	}

	return { useClient, getClient, useQueryArgs, useMutationArgs, handleSubscription };
}
