import type { DataTag } from "@tanstack/query-core";

export type ProcedureKey<TInput, TOutput> = DataTag<
  readonly [path: string, input: TInput],
  TOutput
>;

export type MutationKey<TOutput> = DataTag<readonly [path: string], TOutput>;

export function generateQueryKey<TInput, TOutput>(
  path: string,
  input: TInput,
): ProcedureKey<TInput, TOutput> {
  return [path, input] as any;
}

export function generateMutationKey<TOutput>(
  path: string,
): MutationKey<TOutput> {
  return [path] as any;
}
