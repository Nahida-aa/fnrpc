import type { QueryFunction, QueryFunctionContext, QueryKey } from "@tanstack/query-core";

/**
 * Create a TanStack Query `queryFn` that uses an `AsyncIterable` (SSE
 * subscription) to keep the cache up-to-date.
 *
 * Each emitted chunk replaces the cached value (`setQueryData`). The
 * query resolves (becomes `status: "success"`) after the stream ends,
 * returning the *last* chunk.
 *
 * Use this for "live" data where only the latest value matters.
 *
 * @example
 * ```typescript
 * const options = {
 *   queryKey: fnrpc.clock.liveKey(undefined),
 *   queryFn: liveQuery(({ signal }) => fnrpc.clock.subscribe(undefined, signal)),
 * };
 * ```
 */
export function liveQuery<
  TQueryFnData = unknown,
  TQueryKey extends QueryKey = QueryKey,
>(
  queryFn: (
    context: QueryFunctionContext<TQueryKey>,
  ) => Promise<AsyncIterable<TQueryFnData>>,
): QueryFunction<TQueryFnData, TQueryKey> {
  return async (context) => {
    const stream = await queryFn(context);
    let last: { chunk: TQueryFnData } | undefined;

    for await (const chunk of stream) {
      context.signal?.throwIfAborted();
      last = { chunk };
      context.client.setQueryData<TQueryFnData>(context.queryKey, chunk);
    }

    if (!last) {
      throw new Error(
        `Live query did not yield any data. Ensure the query function returns an AsyncIterable with at least one chunk.`,
      );
    }

    return last.chunk;
  };
}
