import type { Procedure, Procedures } from "@fnrpc/client";

import type { ProcedureUtils } from "./procedure-utils";
import type { StreamedQueryOptions } from "./stream-query";

export type RouterUtils<T extends Procedures> = {
  [K in keyof T]: T[K] extends Procedure
    ? ProcedureUtils<T[K]["input"], T[K]["output"], T[K]["error"]>
    : T[K] extends Procedures
      ? RouterUtils<T[K]>
      : never;
};

export type RouterUtilsOptions<T extends Procedures> = {
  path?: string[];
  scoped?: RouterUtilsScoped<T>;
};

export type RouterUtilsScoped<T extends Procedures> = {
  [K in keyof T]?: T[K] extends Procedure
    ? Partial<ProcedureUtilsOptions>
    : T[K] extends Procedures
      ? RouterUtilsScoped<T[K]>
      : never;
};

export interface ProcedureUtilsOptions {
  queryKey?: (input: unknown) => unknown[];
  mutateKey?: () => unknown[];
}

export type StreamedKeyOptions = {
  queryFnOptions?: StreamedQueryOptions;
};

export type StreamedOptionsIn = {
  queryFnOptions?: StreamedQueryOptions;
};

export type LiveKeyOptions = Record<string, never>;

export type LiveOptionsIn = Record<string, never>;

export type { StreamedQueryOptions } from "./stream-query";
