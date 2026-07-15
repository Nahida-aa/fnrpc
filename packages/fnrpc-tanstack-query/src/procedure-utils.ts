import type { Client } from "@fnrpc/client";
import { traverseClient } from "@fnrpc/client";

import type { DataTag, QueryKey } from "@tanstack/query-core";

import type { MutationKey, ProcedureKey } from "./key";
import { liveQuery } from "./live-query";
import { serializableStreamedQuery } from "./stream-query";
import type { StreamedKeyOptions, StreamedOptionsIn } from "./types";

export class ProcedureUtils<TInput, TOutput, TError> {
  constructor(
    private path: string,
    private client: Client<any>,
  ) {}

  queryKey(input: TInput): ProcedureKey<TInput, TOutput> {
    return [this.path, input] as any;
  }

  queryOptions(input: TInput): {
    queryKey: ProcedureKey<TInput, TOutput>;
    queryFn: () => Promise<TOutput>;
  } {
    return {
      queryKey: this.queryKey(input),
      queryFn: () => this.callClient(input) as Promise<TOutput>,
    };
  }

  mutationKey(): MutationKey<TOutput> {
    return [this.path] as any;
  }

  mutationOptions(): {
    mutationKey: MutationKey<TOutput>;
    mutationFn: (input: TInput) => Promise<TOutput>;
  } {
    return {
      mutationKey: this.mutationKey(),
      mutationFn: (input: TInput) =>
        this.callClient(input) as Promise<TOutput>,
    };
  }

  streamedKey(
    input: TInput,
    options?: StreamedKeyOptions,
  ): DataTag<QueryKey, TOutput[], TError> {
    return [this.path, input, "streamed", options?.queryFnOptions].filter(Boolean) as any;
  }

  streamedOptions<UInput = TInput>(
    input: UInput extends undefined ? void : UInput,
    options?: StreamedOptionsIn,
  ): {
    queryKey: DataTag<QueryKey, TOutput[], TError>;
    queryFn: (context: any) => Promise<TOutput[]>;
  } {
    const queryKey = this.streamedKey(input as any, options);

    return {
      queryKey,
      queryFn: serializableStreamedQuery(
        async (context) => {
          const output = await this.callClient(input, context.signal);
          if (!isAsyncIterable(output)) {
            throw new Error("streamedOptions requires a subscribe procedure (AsyncIterable output)");
          }
          return output;
        },
        options?.queryFnOptions,
      ) as any,
    };
  }

  liveKey(
    input: TInput,
  ): DataTag<QueryKey, TOutput, TError> {
    return [this.path, input, "live"] as any;
  }

  liveOptions<UInput = TInput>(
    input: UInput extends undefined ? void : UInput,
  ): {
    queryKey: DataTag<QueryKey, TOutput, TError>;
    queryFn: (context: any) => Promise<TOutput>;
  } {
    const queryKey = this.liveKey(input as any);

    return {
      queryKey,
      queryFn: liveQuery(
        async (context) => {
          const output = await this.callClient(input, context.signal);
          if (!isAsyncIterable(output)) {
            throw new Error("liveOptions requires a subscribe procedure (AsyncIterable output)");
          }
          return output;
        },
      ) as any,
    };
  }

  private callClient(input: any, signal?: AbortSignal) {
    const segments = this.path.split(".");
    const proxy = traverseClient(this.client, segments);
    return proxy(input, signal);
  }
}

function isAsyncIterable(val: unknown): val is AsyncIterable<unknown> {
  return val !== null && typeof val === "object" && Symbol.asyncIterator in val;
}
