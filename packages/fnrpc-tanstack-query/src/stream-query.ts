import type { QueryFunction, QueryFunctionContext, QueryKey } from "@tanstack/query-core";

export interface StreamedQueryOptions {
  refetchMode?: "append" | "reset" | "replace";
  maxChunks?: number;
}

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

    for await (const chunk of stream) {
      if (context.signal.aborted) {
        throw context.signal.reason;
      }

      result.push(chunk);
      result = limitArraySize(result, maxChunks);

      if (shouldUpdateCacheDuringStream) {
        context.client.setQueryData<Array<TQueryFnData>>(
          context.queryKey,
          (prev = []) => limitArraySize([...prev, chunk], maxChunks),
        );
      }
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
