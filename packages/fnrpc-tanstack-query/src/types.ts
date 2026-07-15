import type { Procedure, Procedures } from "@fnrpc/client";

import type { ProcedureUtils } from "./procedure-utils";

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
