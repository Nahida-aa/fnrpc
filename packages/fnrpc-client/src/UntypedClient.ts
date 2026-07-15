import type { ProcedureKind } from "./types";
import { serialize, flattenForRust, safeStringify } from "./serializer";
import { RpcError } from "./error";

function parseError(msg: string): RpcError {
  try {
    const parsed = JSON.parse(msg);
    return RpcError.fromJson(parsed);
  } catch {
    return new RpcError("INTERNAL_SERVER_ERROR", msg);
  }
}

export const fetchTransport = (config: { url: string }) => {
  return (
    path: string,
    input: unknown,
    kind: ProcedureKind,
    signal?: AbortSignal,
  ): Promise<unknown> => {
    if (kind === "subscribe") {
      return new Promise((resolve, reject) => {
        const serialized = serialize(input);
        const params = new URLSearchParams({
          input: safeStringify(serialized),
        });
        const url = `${config.url}/${path}?${params}`;
        const es = new EventSource(url);

        let done = false;
        const pending: Array<IteratorResult<unknown>> = [];
        let resolveNext: ((r: IteratorResult<unknown>) => void) | null = null;
        let rejectNext: ((err: unknown) => void) | null = null;

        es.onmessage = (e) => {
          if (done) return;
          if (e.data.startsWith("__error:")) {
            const err = parseError(e.data.slice(8));
            done = true;
            es.close();
            if (rejectNext) {
              rejectNext(err);
              rejectNext = null;
            }
          } else {
            try {
              const val = JSON.parse(e.data);
              if (resolveNext) {
                resolveNext({ done: false, value: val });
                resolveNext = null;
              } else {
                pending.push({ done: false, value: val });
              }
            } catch {
              // skip malformed data
            }
          }
        };

        es.onerror = () => {
          done = true;
          es.close();
          const err = new RpcError("CONNECTION_ERROR", "EventSource connection failed");
          if (rejectNext) {
            rejectNext(err);
            rejectNext = null;
          }
        };

        if (signal) {
          signal.addEventListener(
            "abort",
            () => {
              done = true;
              es.close();
              if (resolveNext) {
                resolveNext({ done: true, value: undefined as any });
                resolveNext = null;
              }
            },
            { once: true },
          );
        }

        const iterable: AsyncIterable<unknown> = {
          [Symbol.asyncIterator]() {
            return {
              next(): Promise<IteratorResult<unknown>> {
                if (done) {
                  return Promise.resolve({ done: true, value: undefined as any });
                }
                if (pending.length > 0) {
                  return Promise.resolve(pending.shift()!);
                }
                return new Promise((res, rej) => {
                  resolveNext = res;
                  rejectNext = rej;
                });
              },
              return(): Promise<IteratorResult<unknown>> {
                done = true;
                es.close();
                if (resolveNext) {
                  resolveNext({ done: true, value: undefined as any });
                  resolveNext = null;
                }
                return Promise.resolve({ done: true, value: undefined as any });
              },
            };
          },
        } satisfies AsyncIterable<unknown>;

        es.onopen = () => resolve(iterable);
        es.onerror = () => {
          reject(new RpcError("CONNECTION_ERROR", "EventSource connection failed"));
          es.close();
        };
      });
    }

    // query / mutate
    const isQuery = kind === "query";
    const serialized = serialize(input);
    const body = safeStringify(flattenForRust(serialized));

    if (isQuery) {
      const params = new URLSearchParams({ input: body });
      return fetch(`${config.url}/${path}?${params}`, {
        method: "GET",
        headers: { Accept: "application/json" },
        signal,
      }).then(async (r) => {
        if (!r.ok) {
          const json = await r.json().catch(() => null);
          if (json && typeof json.code === "string") {
            throw RpcError.fromJson(json);
          }
          throw new RpcError("INTERNAL_SERVER_ERROR", `Request failed: ${r.status}`);
        }
        return r.json();
      });
    }

    return fetch(`${config.url}/${path}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
      body,
      signal,
    }).then(async (r) => {
      if (!r.ok) {
        const json = await r.json().catch(() => null);
        if (json && typeof json.code === "string") {
          throw RpcError.fromJson(json);
        }
        throw new RpcError("INTERNAL_SERVER_ERROR", `Request failed: ${r.status}`);
      }
      return r.json();
    });
  };
};

export function consumeEventIterator<T, E = RpcError>(
  iterable: AsyncIterable<T> | Promise<AsyncIterable<T>>,
  opts: {
    onEvent?: (value: T) => void;
    onError?: (err: E) => void;
    onComplete?: () => void;
    onFinish?: () => void;
  },
  signal?: AbortSignal,
): () => void {
  let cancelled = false;

  let iterator: AsyncIterator<T>;

  async function run() {
    try {
      const resolved = await iterable;
      iterator = resolved[Symbol.asyncIterator]();

      while (!cancelled) {
        const { done, value } = await iterator.next();
        if (done || cancelled) break;
        opts.onEvent?.(value as T);
      }
      if (!cancelled) {
        opts.onComplete?.();
      }
    } catch (err) {
      if (!cancelled) {
        opts.onError?.(err as E);
      }
    } finally {
      if (!cancelled) {
        opts.onFinish?.();
      }
    }
  }

  run();

  if (signal) {
    signal.addEventListener(
      "abort",
      () => {
        cancelled = true;
        iterator?.return?.();
      },
      { once: true },
    );
  }

  return () => {
    cancelled = true;
    iterator?.return?.();
  };
}
