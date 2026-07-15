import type { ProcedureKind, Procedure, Procedures } from "./types";

export type ProcedureWithKind<V extends ProcedureKind> = Omit<
  Procedure,
  "kind"
> & { kind: V };

export type VoidIfInputNull<
  P extends Procedure,
  Else = P["input"],
> = P["input"] extends null ? void : Else;

export type ProcedureCallable<P extends Procedure> =
  P["kind"] extends "subscribe"
    ? (input: VoidIfInputNull<P>, signal?: AbortSignal) => Promise<AsyncIterable<P["output"]>>
    : (input: VoidIfInputNull<P>, signal?: AbortSignal) => Promise<P["output"]>;

type ClientProceduresProxy<P extends Procedures> = {
  [K in keyof P]: P[K] extends Procedure
    ? ProcedureCallable<P[K]>
    : P[K] extends Procedures
      ? ClientProceduresProxy<P[K]>
      : never;
};

export type Client<P extends Procedures> = ClientProceduresProxy<P>;

type Transport = (
  path: string,
  input: unknown,
  kind: ProcedureKind,
  signal?: AbortSignal,
  method?: string,
) => Promise<unknown>;

type ProcedureMeta = {
  kind: ProcedureKind;
  method: string;
};

const noop = () => {
  // noop
};

export function createProceduresProxy<T>(
  callback: (opts: { path: string[]; args: any[] }) => unknown,
  path: string[] = [],
): T {
  return new Proxy(noop, {
    get(_, key) {
      if (typeof key !== "string") return;

      return createProceduresProxy(callback, [...path, key]);
    },
    apply(_1, _2, args) {
      return callback({ args, path });
    },
  }) as T;
}

export function createClient<P extends Procedures>(
  transport: Transport,
  metaMap: Record<string, ProcedureMeta>,
): Client<P> {
  return createProceduresProxy<Client<P>>(({ args, path }) => {
    const pathStr = path.join(".");
    const meta = metaMap[pathStr];
    console.debug('transport args:', pathStr, 'input:', args[0], 'kind:', meta.kind, 'signal:', args[1], 'method:', meta.method);
    if (!meta) throw new Error(`Unknown procedure: ${pathStr}`);
    return transport(pathStr, args[0], meta.kind, args[1], meta.method);
  });
}

export function getQueryKey(
  path: string,
  input: unknown,
): [string] | [string, unknown] {
  return input === undefined ? [path] : [path, input];
}

export function traverseClient(
  client: Client<any>,
  path: string[],
): (...args: any[]) => any {
  let ret: any = client;

  for (const segment of path) {
    ret = ret[segment];
  }

  return ret as any;
}
