import type { QueryFunction, QueryFunctionContext, QueryKey } from "@tanstack/query-core";

/**
 * Behaviour when a streamed query refetches.
 *
 * - `"reset"` — clear cached data and start fresh.
 * - `"append"` — keep existing entries and append new ones.
 * - `"replace"` — collect all entries in memory, then swap cache at the end.
 */
export interface StreamedQueryOptions {
  refetchMode?: "append" | "reset" | "replace";
  /** Maximum number of chunks to retain (oldest are dropped). */
  maxChunks?: number;
}

/**
 * Create a TanStack Query `queryFn` that collects `AsyncIterable` chunks
 * into an array and progressively updates the cache.
 *
 * Use this for procedures that emit a stream of values you want to display
 * as a list (e.g. log entries, search results).
 *
 * @example
 * ```typescript
 * const options = {
 *   queryKey: fnrpc.events.streamedKey(undefined),
 *   queryFn: serializableStreamedQuery(
 *     ({ signal }) => fnrpc.events.subscribe(undefined, signal),
 *     { refetchMode: "append" },
 *   ),
 * };
 * ```
 */
export function serializableStreamedQuery<
  TQueryFnData = unknown,
  TQueryKey extends QueryKey = QueryKey,
>(
  queryFn: (
    context: QueryFunctionContext<TQueryKey>,
  ) => Promise<AsyncIterable<TQueryFnData>>,
  { refetchMode = "reset", maxChunks = Number.POSITIVE_INFINITY }: StreamedQueryOptions = {},
): QueryFunction<TQueryFnData[], TQueryKey> {
  return async (context) => {
    const query = context.client
      .getQueryCache()
      .find({ queryKey: context.queryKey, exact: true });
    const hasPreviousData = !!query && query.state.data !== undefined;

    if (hasPreviousData) {
      if (refetchMode === "reset") {
        query!.setState({
          status: "pending",
          data: undefined,
          error: null,
          fetchStatus: "fetching",
        });
      } else {
        context.client.setQueryData<Array<TQueryFnData>>(
          context.queryKey,
          (prev = []) => limitArraySize(prev, maxChunks),
        );
      }
    }

    let result: Array<TQueryFnData> = [];
    const stream = await queryFn(context);
    const shouldUpdateCacheDuringStream = !hasPreviousData || refetchMode !== "replace";

    context.client.setQueryData<Array<TQueryFnData>>(
      context.queryKey,
      (prev = []) => limitArraySize(prev, maxChunks),
    );

    const iterator = stream[Symbol.asyncIterator]();
    let done = false;
    const abortHandler = () => {
      if (!done) {
        done = true;
        iterator.return?.();
      }
    };
    context.signal?.addEventListener("abort", abortHandler, { once: true });

    try {
      for await (const chunk of { [Symbol.asyncIterator]() { return iterator; } }) {
        if (context.signal.aborted) break;

        result.push(chunk);
        result = limitArraySize(result, maxChunks);

        if (shouldUpdateCacheDuringStream) {
          context.client.setQueryData<Array<TQueryFnData>>(
            context.queryKey,
            (prev = []) => limitArraySize([...prev, chunk], maxChunks),
          );
        }
      }
    } finally {
      done = true;
      context.signal?.removeEventListener("abort", abortHandler);
    }

    if (!shouldUpdateCacheDuringStream) {
      context.client.setQueryData<Array<TQueryFnData>>(context.queryKey, result);
    }

    const cachedData = context.client.getQueryData<Array<TQueryFnData>>(context.queryKey);
    if (cachedData) {
      return limitArraySize(cachedData, maxChunks);
    }

    return result;
  };
}

function limitArraySize<T>(items: Array<T>, maxSize: number): Array<T> {
  if (items.length <= maxSize) return items;
  return items.slice(items.length - maxSize);
}
