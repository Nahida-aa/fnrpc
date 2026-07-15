export type JoinPath<
  TPath extends string,
  TNext extends string,
> = TPath extends "" ? TNext : `${TPath}.${TNext}`;

export type ProcedureKind = "query" | "mutate" | "subscribe";

export type Procedure = {
  kind: ProcedureKind;
  input: unknown;
  output: unknown;
  error: { code: string; message: string; data?: unknown };
};

export type Procedures = {
  [K in string]: Procedure | Procedures;
};

export type Result<Ok, Err> =
  | { status: "ok"; data: Ok }
  | { status: "err"; error: Err };

export type ProcedureResult<P extends Procedure> = Result<
  P["output"],
  P["error"]
>;

export type ConsumeEventOptions<T, E> = {
  onEvent?: (value: T) => void;
  onError?: (err: E) => void;
  onComplete?: () => void;
  onFinish?: () => void;
};
