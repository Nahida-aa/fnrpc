import type {
  Client,
  Procedures,
} from "@fnrpc/client";
import { traverseClient, getQueryKey } from "@fnrpc/client";
import type * as tanstack from "@tanstack/query-core";

/**
 * Call a query procedure by string path.
 *
 * @internal
 */
export function callQuery<P extends Procedures, K extends keyof P & string>(
  client: Client<P>,
  path: K,
  input: P[K]["input"],
): Promise<P[K]["output"]> {
  const segments = (path as string).split(".");
  const proxy = traverseClient(client, segments);
  return proxy(input) as Promise<P[K]["output"]>;
}

/**
 * Call a mutate procedure by string path.
 *
 * @internal
 */
export function callMutation<P extends Procedures, K extends keyof P & string>(
  client: Client<P>,
  path: K,
  input: P[K]["input"],
): Promise<P[K]["output"]> {
  const segments = (path as string).split(".");
  const proxy = traverseClient(client, segments);
  return proxy(input) as Promise<P[K]["output"]>;
}

/**
 * Low-level utility functions for working with TanStack Query cache
 * using procedure paths.
 *
 * Provides `fetch`, `prefetch`, `ensureData`, `invalidate`, `refetch`,
 * `cancel`, `setData`, and `getData` — all keyed by procedure path + input.
 *
 * @deprecated Use [`createRouterUtils`] + [`ProcedureUtils`] instead,
 * which provide a more ergonomic typed API.
 */
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
      filters?: Omit<tanstack.InvalidateQueryFilters, "queryKey" | "predicate">,
      opts?: tanstack.InvalidateOptions,
    ) =>
      queryClient.invalidateQueries(
        {
          ...filters,
          predicate: (query) => {
            const key = query.queryKey[0] as unknown;
            return (
              typeof key === "string" &&
              (key === path || key.startsWith(path + "."))
            );
          },
        },
        opts,
      ),

    refetch: <T extends K>(
      path: T,
      filters?: Omit<tanstack.RefetchQueryFilters, "queryKey" | "predicate">,
      opts?: tanstack.RefetchOptions,
    ) =>
      queryClient.refetchQueries(
        {
          ...filters,
          predicate: (query) => {
            const key = query.queryKey[0] as unknown;
            return (
              typeof key === "string" &&
              (key === path || key.startsWith(path + "."))
            );
          },
        },
        opts,
      ),

    cancel: <T extends K>(
      path: T,
      filters?: Omit<tanstack.QueryFilters, "queryKey" | "predicate">,
      opts?: tanstack.CancelOptions,
    ) =>
      queryClient.cancelQueries(
        {
          ...filters,
          predicate: (query) => {
            const key = query.queryKey[0] as unknown;
            return (
              typeof key === "string" &&
              (key === path || key.startsWith(path + "."))
            );
          },
        },
        opts,
      ),

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

    getData: <T extends K>(path: T, input: P[T]["input"]) =>
      queryClient.getQueryData<P[T]["output"]>(
        getQueryKey(path as string, input),
      ),
  };
}
