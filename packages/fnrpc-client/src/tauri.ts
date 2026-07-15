import type { ProcedureKind } from "./types";

export interface TauriCore {
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
  Channel: new <T = unknown>() => { onmessage: ((msg: T) => void) | null };
}

export function tauriTransport(getCore: () => Promise<TauriCore>) {
  return (
    path: string,
    input: unknown,
    kind: ProcedureKind,
    signal?: AbortSignal,
  ): Promise<unknown> | AsyncIterable<unknown> => {
    if (kind === "subscribe") {
      const iterable: AsyncIterable<unknown> = {
        [Symbol.asyncIterator]() {
          let done = false;
          const pending: Array<IteratorResult<unknown>> = [];
          let resolve: ((r: IteratorResult<unknown>) => void) | null = null;

          void getCore()
            .then(async (mod) => {
              const channel = new mod.Channel<string>();
              channel.onmessage = (msg: string) => {
                if (done) return;
                if (msg.startsWith("__error:")) {
                  push({ done: true as const, value: undefined as any });
                } else {
                  push({ done: false as const, value: msg });
                }
              };

              await mod.invoke("rpc_sub", {
                path,
                input: input ?? null,
                channel,
              }).catch(() => {
                push({ done: true as const, value: undefined as any });
              });
            })
            .catch(() => {
              push({ done: true as const, value: undefined as any });
            });

          function push(result: IteratorResult<unknown>) {
            if (resolve) {
              resolve(result);
              resolve = null;
            } else {
              pending.push(result);
            }
          }

          if (signal) {
            signal.addEventListener("abort", () => {
              done = true;
            }, { once: true });
          }

          return {
            next(): Promise<IteratorResult<unknown>> {
              if (done) {
                return Promise.resolve({ done: true, value: undefined as any });
              }
              if (pending.length > 0) {
                return Promise.resolve(pending.shift()!);
              }
              return new Promise((res) => {
                resolve = res;
              });
            },
            return(): Promise<IteratorResult<unknown>> {
              done = true;
              if (resolve) {
                resolve({ done: true, value: undefined as any });
              }
              return Promise.resolve({ done: true, value: undefined as any });
            },
          };
        },
      };

      return iterable;
    }

    return getCore()
      .then((mod) =>
        mod.invoke("rpc_fn", {
          path,
          input: input ?? null,
        }),
      );
  };
}
