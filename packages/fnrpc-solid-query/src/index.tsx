import type { Client, Procedures } from "@fnrpc/client";
import * as solid from "solid-js";
import * as tanstackQuery from "@fnrpc/tanstack-query";
import * as tanstack from "@tanstack/solid-query";

export function createSolidQueryHooks<P extends Procedures>(
	ctx: tanstackQuery.Context<P>,
) {
	type K = keyof P & string;

	function useUtils() {
		if (!ctx.client || !ctx.queryClient)
			throw new Error(
				"The fnrpc context has not been set. Ensure the <fnrpc.Provider> component is higher up in your component tree.",
			);
		return tanstackQuery.createUtils(ctx.client, ctx.queryClient);
	}

	function createQuery<T extends K>(
		keyAndInput: solid.Accessor<
			tanstackQuery.QueryKeyAndInput<P, T> | [T, typeof tanstackQuery.skipToken]
		>,
		opts?: solid.Accessor<
			Omit<
				tanstack.CreateQueryOptions<
					P[T]["output"],
					Error,
					P[T]["output"],
					tanstackQuery.QueryKeyAndInput<P, T>
				>,
				"queryKey" | "queryFn"
			> & {
				initialData: 
					| P[T]["output"]
					| (()=> P[T]["output"])
			}
		> ,
	): tanstack.CreateQueryResult<P[T]["output"], Error> {
		return (tanstack.createQuery)(() => {
			const args = keyAndInput();
			const [key, ...rest] = args;
			const opts_ = () => opts?.() ?? { initialData: undefined }; 
			if (rest[0] === tanstackQuery.skipToken) {
				return tanstack.queryOptions({ ...opts_(), 
					queryKey: args as [T] | [T, P[T]["input"]], 
					queryFn: tanstackQuery.skipToken, enabled: false });
			}

			const input = rest[0] as P[T]["input"];
			return tanstack.queryOptions({ 
				...opts_(), 
				...tanstackQuery.createQueryOptions(ctx.client, key, input) 
			});
		}) as tanstack.CreateQueryResult<P[T]["output"], Error>;
	}

	function createMutation<T extends K>(
		key: solid.Accessor<T>,
		opts?: solid.Accessor<
			Omit<
				tanstack.CreateMutationOptions<
					P[T]["output"],
					Error,
					P[T]["input"],
					Error
				>,
				"mutationKey" | "mutationFn"
			>
		>,
	) {
		return tanstack.createMutation(() => ({
			...opts?.(),
			mutationKey: [key()],
			mutationFn: (input: P[T]["input"]) =>
				tanstackQuery.callMutation(ctx.client, key(), input),
		}));
	}

	function createSubscription<T extends K>(
		keyAndInput: () => tanstackQuery.QueryKeyAndInput<P, T> | [T, typeof tanstackQuery.skipToken],
		opts: () => tanstackQuery.SubscriptionOptions<P, T>,
	) {
		solid.createEffect(
			solid.on(
				() => [keyAndInput(), opts()] as const,
				([ki, options]) => {
					const [key, ...rest] = ki;
					const input = rest[0] as P[T]["input"] | undefined;

					if (!(options.enabled ?? true) || rest[0] === tanstackQuery.skipToken) return;

					const client = options.client ?? ctx.client;
					let isStopped = false;

					const segments = (key as string).split(".");
					let proxy: any = client as any;
					for (const segment of segments) {
						proxy = proxy[segment];
					}

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

					solid.onCleanup(() => {
						isStopped = true;
					});
				},
			),
		);
	}

	return {
		Provider: (props: {
			children?: solid.JSX.Element;
			client: Client<P>;
			queryClient: tanstack.QueryClient;
		}): solid.JSX.Element => {
			ctx.client = props.client;
			ctx.queryClient = props.queryClient;
			return props.children;
		},
		useUtils,
		createQuery,
		createMutation,
		createSubscription,
	};
}
