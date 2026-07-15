import type { Client } from "@fnrpc/client";
import { traverseClient } from "@fnrpc/client";

import type { DataTag, QueryKey, QueryObserverOptions, MutationObserverOptions } from "@tanstack/query-core";

import type { MutationKey, ProcedureKey } from "./key";
import { liveQuery } from "./live-query";
import { serializableStreamedQuery } from "./stream-query";
import type { ExtraLiveOptions, ExtraStreamedOptions, StreamedKeyOptions } from "./types";

function sanitizeVal(val: unknown): unknown {
  if (typeof val === "bigint") return `${val}n`;
  if (Array.isArray(val)) return val.map(sanitizeVal);
  if (val !== null && typeof val === "object") {
    const obj: Record<string, unknown> = {};
    for (const k of Object.keys(val as Record<string, unknown>)) {
      obj[k] = sanitizeVal((val as Record<string, unknown>)[k]);
    }
    return obj;
  }
  return val;
}

export class ProcedureUtils<TInput, TOutput, TError> {
  constructor(
    private path: string,
    private client: Client<any>,
  ) {}

  queryKey(input: TInput): ProcedureKey<TInput, TOutput> {
    return [this.path, sanitizeVal(input)] as any;
  }

  queryOptions<UInput = TInput>(
    input: UInput extends undefined ? void : UInput,
    opts?: Omit<QueryObserverOptions<TOutput, TError, TOutput, TOutput, QueryKey>, "queryKey" | "queryFn" | "initialData">,
  ): {
    queryKey: ProcedureKey<TInput, TOutput>;
    queryFn: () => Promise<TOutput>;
  } & Omit<QueryObserverOptions<TOutput, TError, TOutput, TOutput, QueryKey>, "queryKey" | "queryFn" | "initialData"> {
    return {
      queryKey: this.queryKey(input as any),
      queryFn: () => this.call(input, undefined) as Promise<TOutput>,
      ...opts,
    } as any;
  }

  mutationKey(): MutationKey<TOutput> {
    return [this.path] as any;
  }

  mutationOptions(
    opts?: Omit<MutationObserverOptions<TOutput, TError, TInput, unknown>, "mutationKey" | "mutationFn">,
  ): {
    mutationKey: MutationKey<TOutput>;
    mutationFn: (input: TInput) => Promise<TOutput>;
  } & Omit<MutationObserverOptions<TOutput, TError, TInput, unknown>, "mutationKey" | "mutationFn"> {
    return {
      mutationKey: this.mutationKey(),
      mutationFn: (input: TInput) =>
        this.call(input, undefined) as Promise<TOutput>,
      ...opts,
    } as any;
  }

  streamedKey(
    input: TInput,
    options?: StreamedKeyOptions,
  ): DataTag<QueryKey, TOutput[], TError> {
    return [this.path, sanitizeVal(input), "streamed", options?.queryFnOptions].filter(Boolean) as any;
  }

  streamedOptions<UInput = TInput>(
    input: UInput extends undefined ? void : UInput,
    options?: ExtraStreamedOptions<TOutput, TError>,
  ): {
    queryKey: DataTag<QueryKey, TOutput[], TError>;
    queryFn: (context: any) => Promise<TOutput[]>;
  } & ExtraStreamedOptions<TOutput, TError> {
    const queryFnOpts = options?.queryFnOptions;
    const tanstackOpts: Omit<ExtraStreamedOptions<TOutput, TError>, "queryFnOptions"> = options ?? {};

    return {
      queryKey: this.streamedKey(input as any, { queryFnOptions: queryFnOpts }),
      queryFn: serializableStreamedQuery(
        async (context) => {
          const output = await this.call(input, context.signal);
          if (!isAsyncIterable(output)) {
            throw new Error("streamedOptions requires a subscribe procedure (AsyncIterable output)");
          }
          return output;
        },
        queryFnOpts,
      ) as any,
      ...tanstackOpts,
    } as any;
  }

  liveKey(
    input: TInput,
  ): DataTag<QueryKey, TOutput, TError> {
    return [this.path, sanitizeVal(input), "live"] as any;
  }

  liveOptions<UInput = TInput>(
    input: UInput extends undefined ? void : UInput,
    options?: ExtraLiveOptions<TOutput, TError>,
  ): {
    queryKey: DataTag<QueryKey, TOutput, TError>;
    queryFn: (context: any) => Promise<TOutput>;
  } & ExtraLiveOptions<TOutput, TError> {
    const extras = options ?? {};
    return {
      queryKey: this.liveKey(input as any),
      queryFn: liveQuery(
        async (context) => {
          const output = await this.call(input, context.signal);
          if (!isAsyncIterable(output)) {
            throw new Error("liveOptions requires a subscribe procedure (AsyncIterable output)");
          }
          return output;
        },
      ) as any,
      ...extras,
    } as any;
  }

  call(input: any, signal?: AbortSignal) {
    const segments = this.path.split(".");
    const proxy = traverseClient(this.client, segments);
    return proxy(input, signal);
  }
}

function isAsyncIterable(val: unknown): val is AsyncIterable<unknown> {
  return val !== null && typeof val === "object" && Symbol.asyncIterator in val;
}
