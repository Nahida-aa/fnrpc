import type { ProcedureKind } from "./types"
import { serialize, flattenForRust, safeStringify } from "./serializer"
import { RpcError } from "./error"
import { connectSSE } from "./sse"

function parseError(msg: string): RpcError {
  try {
    const parsed = JSON.parse(msg)
    return RpcError.fromJson(parsed)
  } catch {
    return new RpcError("INTERNAL_SERVER_ERROR", msg)
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/**
 * Create an HTTP fetch transport for fnrpc.
 *
 * - Queries: `GET /<path>?input=...`
 * - Mutations: `POST /<path>` with JSON body
 * - Subscriptions: SSE stream via [`createSSEIterable`]
 *
 * @example
 * ```typescript
 * const transport = fetchTransport({ url: "http://localhost:3000/fnrpc" });
 * ```
 */
export const fetchTransport = (config: { url: string }) => {
  return (
    path: string,
    input: unknown,
    kind: ProcedureKind,
    signal?: AbortSignal,
    method?: string,
  ): Promise<unknown> => {
    if (kind === "subscribe") {
      return createSSEIterable(config.url, path, input, signal, (method || "GET") as "GET" | "POST")
    }

    // query / mutate
    const isQuery = kind === "query"
    const serialized = serialize(input)
    const body = safeStringify(flattenForRust(serialized))

    if (isQuery) {
      const params = new URLSearchParams({ input: body })
      return fetch(`${config.url}/${path}?${params}`, {
        method: "GET",
        headers: { Accept: "application/json" },
        signal,
      }).then(async (r) => {
        if (!r.ok) {
          const json = await r.json().catch(() => null)
          if (json && typeof json.code === "string") {
            throw RpcError.fromJson(json)
          }
          throw new RpcError("INTERNAL_SERVER_ERROR", `Request failed: ${r.status}`)
        }
        return r.json()
      })
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
        const json = await r.json().catch(() => null)
        if (json && typeof json.code === "string") {
          throw RpcError.fromJson(json)
        }
        throw new RpcError("INTERNAL_SERVER_ERROR", `Request failed: ${r.status}`)
      }
      return r.json()
    })
  }
}

function createSSEIterable(
  baseUrl: string,
  path: string,
  input: unknown,
  signal?: AbortSignal,
  method: "GET" | "POST" = "GET",
): Promise<AsyncIterable<unknown>> {
  const serialized = serialize(input)
  let url: string
  let body: string | undefined

  if (method === "POST") {
    body = safeStringify(flattenForRust(serialized))
    url = `${baseUrl}/${path}`
  } else {
    const params = new URLSearchParams({ input: safeStringify(serialized) })
    url = `${baseUrl}/${path}?${params}`
  }

  let aborted = false
  if (signal) {
    signal.addEventListener("abort", () => {
      aborted = true
      closeStream?.()
      if (rejectNext) {
        rejectNext((signal as AbortSignal).reason)
        rejectNext = null
      } else if (resolveNext) {
        resolveNext({ done: true, value: undefined as any })
        resolveNext = null
      }
    }, { once: true })
  }

  const pending: Array<IteratorResult<unknown>> = []
  let resolveNext: ((r: IteratorResult<unknown>) => void) | null = null
  let rejectNext: ((err: unknown) => void) | null = null
  let lastEventId: string | undefined
  let closed = false
  let closeStream: (() => void) | undefined

  async function pump() {
    let retryDelay = 1000

    while (!aborted && !closed) {
      try {
        const { iterable, close } = await connectSSE({ url, signal, lastEventId, body, method })
        closeStream = close

        retryDelay = 1000

        for await (const event of iterable) {
          if (aborted || closed) {
            close()
            return
          }

          if (event.id) lastEventId = event.id

          if (event.data.startsWith("__error:")) {
            closed = true
            close()
            const err = parseError(event.data.slice(8))
            if (rejectNext) {
              rejectNext(err)
              rejectNext = null
            }
            return
          }

          let val: unknown
          try {
            val = JSON.parse(event.data)
          } catch {
            continue
          }

          if (resolveNext) {
            resolveNext({ done: false, value: val })
            resolveNext = null
          } else {
            pending.push({ done: false, value: val })
          }
        }
      } catch (err) {
        if (aborted || closed) return
        // connection dropped — retry
      }

      if (aborted || closed) return

      await sleep(retryDelay)
      retryDelay = Math.min(retryDelay * 2, 30000)
    }
  }

  pump()

  const iterable: AsyncIterable<unknown> = {
    [Symbol.asyncIterator]() {
      return {
        next(): Promise<IteratorResult<unknown>> {
          if (closed) {
            return Promise.resolve({ done: true, value: undefined as any })
          }
          if (pending.length > 0) {
            return Promise.resolve(pending.shift()!)
          }
          return new Promise((res, rej) => {
            resolveNext = res
            rejectNext = rej
          })
        },
        return(): Promise<IteratorResult<unknown>> {
          closed = true
          closeStream?.()
          if (resolveNext) {
            resolveNext({ done: true, value: undefined as any })
            resolveNext = null
          }
          return Promise.resolve({ done: true, value: undefined as any })
        },
      }
    },
  } satisfies AsyncIterable<unknown>

  return Promise.resolve(iterable)
}

/**
 * Consume an async iterable (SSE subscription) with callbacks.
 *
 * Useful when you want to subscribe to events with imperative lifecycle
 * handlers instead of `for await`.
 *
 * @returns A cancel function to stop consuming.
 *
 * @example
 * ```typescript
 * const cancel = consumeEventIterator(fnrpc.events.onMessage(null), {
 *   onEvent: (msg) => console.log("received", msg),
 *   onError: (err) => console.error("error", err),
 * });
 *
 * // later:
 * cancel();
 * ```
 */
export function consumeEventIterator<T, E = RpcError>(
  iterable: AsyncIterable<T> | Promise<AsyncIterable<T>>,
  opts: {
    onEvent?: (value: T) => void
    onError?: (err: E) => void
    onComplete?: () => void
    onFinish?: () => void
  },
  signal?: AbortSignal,
): () => void {
  let cancelled = false

  let iterator: AsyncIterator<T>

  async function run() {
    try {
      const resolved = await iterable
      iterator = resolved[Symbol.asyncIterator]()

      while (!cancelled) {
        const { done, value } = await iterator.next()
        if (done || cancelled) break
        opts.onEvent?.(value as T)
      }
      if (!cancelled) {
        opts.onComplete?.()
      }
    } catch (err) {
      if (!cancelled) {
        opts.onError?.(err as E)
      }
    } finally {
      if (!cancelled) {
        opts.onFinish?.()
      }
    }
  }

  run()

  if (signal) {
    signal.addEventListener(
      "abort",
      () => {
        cancelled = true
        iterator?.return?.()
      },
      { once: true },
    )
  }

  return () => {
    cancelled = true
    iterator?.return?.()
  }
}
