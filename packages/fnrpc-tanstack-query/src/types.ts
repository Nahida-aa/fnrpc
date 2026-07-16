import type { Procedure, Procedures } from "@fnrpc/client";
import type { QueryKey, QueryObserverOptions } from "@tanstack/query-core";

import type { ProcedureUtils } from "./procedure-utils";
import type { StreamedQueryOptions } from "./stream-query";

/**
 * Mirror of the `Procedures` shape where each leaf procedure is
 * replaced by its [`ProcedureUtils`] helper.
 *
 * Generated at runtime by [`createRouterUtils`].
 */
export type RouterUtils<T extends Procedures> = {
  [K in keyof T]: T[K] extends Procedure
    ? ProcedureUtils<T[K]["input"], T[K]["output"], T[K]["error"]>
    : T[K] extends Procedures
      ? RouterUtils<T[K]>
      : never;
};

/** Options for [`createRouterUtils`]. */
export type RouterUtilsOptions<T extends Procedures> = {
  path?: string[];
  scoped?: RouterUtilsScoped<T>;
};

/**
 * Partial overrides for scoped subsets of a router.
 *
 * Each key mirrors a sub-router or procedure and can supply partial
 * [`ProcedureUtilsOptions`].
 */
export type RouterUtilsScoped<T extends Procedures> = {
  [K in keyof T]?: T[K] extends Procedure
    ? Partial<ProcedureUtilsOptions>
    : T[K] extends Procedures
      ? RouterUtilsScoped<T[K]>
      : never;
};

/** Per-procedure overrides for query/mutation key generators. */
export interface ProcedureUtilsOptions {
  queryKey?: (input: unknown) => unknown[];
  mutateKey?: () => unknown[];
}

/** Options for a streamed query key. */
export type StreamedKeyOptions = {
  queryFnOptions?: StreamedQueryOptions;
};

/**
 * Extra options for a streamed TanStack Query, omitting key/fn/data fields
 * that are filled automatically.
 */
export type ExtraStreamedOptions<TOutput, TError> =
  Omit<
    QueryObserverOptions<TOutput[], TError, TOutput[], TOutput[], QueryKey>,
    "queryKey" | "queryFn" | "initialData" | "_defaulted" | "_optimisticResults"
  > & {
    queryFnOptions?: StreamedQueryOptions;
  };

/**
 * Extra options for a live TanStack Query, omitting key/fn/data fields
 * that are filled automatically.
 */
export type ExtraLiveOptions<TOutput, TError> =
  Omit<
    QueryObserverOptions<TOutput, TError, TOutput, TOutput, QueryKey>,
    "queryKey" | "queryFn" | "initialData" | "_defaulted" | "_optimisticResults"
  >;

export type { StreamedQueryOptions } from "./stream-query";
