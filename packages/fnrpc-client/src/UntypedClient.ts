import type { ProcedureKind } from "./types";

// JSON.stringify throws on BigInt values. This replacer converts BigInt to
// Number so JSON transport works. Values > 2^53 lose precision — acceptable
// for this transport (JSON has no native bigint type; the Rust backend
// receives a JSON number either way).
function safeStringify(value: unknown): string {
  return JSON.stringify(value, (_, val) =>
    typeof val === "bigint" ? Number(val) : val,
  );
}

export const fetchTransport = (
  config: { url: string },
) => {
  return (
    path: string,
    input: unknown,
    kind: ProcedureKind,
    signal?: AbortSignal,
  ): Promise<unknown> | AsyncIterable<unknown> => {
    if (kind === "subscribe") {
      // GET → SSE
      const params = new URLSearchParams({
        input: safeStringify(input),
      });
      const url = `${config.url}/${path}?${params}`;
      const es = new EventSource(url);

      const iterable: AsyncIterable<unknown> = {
        [Symbol.asyncIterator]() {
          let done = false;
          const pending: Array<IteratorResult<unknown>> = [];
          let resolve: ((r: IteratorResult<unknown>) => void) | null = null;

          es.onmessage = (e) => {
            if (done) return;
            try {
              const val = JSON.parse(e.data);
              push({ done: false, value: val });
            } catch {
              // skip malformed data
            }
          };

          es.onerror = () => {
            done = true;
            es.close();
            push({ done: true, value: undefined as any });
          };

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
              es.close();
              if (resolve) {
                resolve({ done: true, value: undefined as any });
              }
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
              es.close();
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

    // query / mutate
    const isQuery = kind === "query";

    if (isQuery) {
      const params = new URLSearchParams({
        input: safeStringify(input),
      });
      return fetch(`${config.url}/${path}?${params}`, {
        method: "GET",
        headers: { Accept: "application/json" },
        signal,
      }).then((r) => {
        if (!r.ok) throw new Error(`Request failed: ${r.status}`);
        return r.json();
      });
    }

    // mutate
    return fetch(`${config.url}/${path}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
      body: safeStringify(input),
      signal,
    }).then((r) => {
      if (!r.ok) throw new Error(`Request failed: ${r.status}`);
      return r.json();
    });
  };
};

export function consumeEventIterator<T, E = Error>(
  iterable: AsyncIterable<T>,
  opts: {
    onEvent?: (value: T) => void;
    onError?: (err: E) => void;
    onComplete?: () => void;
    onFinish?: () => void;
  },
  signal?: AbortSignal,
): () => void {
  let cancelled = false;

  const iterator = iterable[Symbol.asyncIterator]();

  async function run() {
    try {
      while (!cancelled) {
        const { done, value } = await iterator.next();
        if (done || cancelled) break;
        opts.onEvent?.(value);
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
    signal.addEventListener("abort", () => {
      cancelled = true;
      iterator.return?.();
    }, { once: true });
  }

  return () => {
    cancelled = true;
    iterator.return?.();
  };
}
