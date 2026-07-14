import type { Client } from "@fnrpc/client";

import type { MutationKey, ProcedureKey } from "./key";
import { callMutation, callQuery } from "./utils";

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
      queryFn: () =>
        callQuery(this.client, this.path as any, input) as Promise<TOutput>,
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
        callMutation(this.client, this.path as any, input) as Promise<TOutput>,
    };
  }
}
