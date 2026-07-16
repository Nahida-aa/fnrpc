import type { DataTag } from "@tanstack/query-core";

/**
 * TanStack Query key for a query/mutate procedure, tagged with the output type.
 *
 * @example `["users.get", { id: 1 }]`
 */
export type ProcedureKey<TInput, TOutput> = DataTag<
  readonly [path: string, input: TInput],
  TOutput
>;

/**
 * TanStack Query key for a mutation, tagged with the output type.
 *
 * @example `["users.create"]`
 */
export type MutationKey<TOutput> = DataTag<readonly [path: string], TOutput>;

/**
 * Generate a typed query key for a procedure.
 *
 * @param path - Dot-separated procedure path.
 * @param input - The input value (or `undefined` if no input).
 */
export function generateQueryKey<TInput, TOutput>(
  path: string,
  input: TInput,
): ProcedureKey<TInput, TOutput> {
  return [path, input] as any;
}

/**
 * Generate a typed mutation key for a procedure.
 *
 * @param path - Dot-separated procedure path.
 */
export function generateMutationKey<TOutput>(
  path: string,
): MutationKey<TOutput> {
  return [path] as any;
}
