/** A single SSE event parsed from the wire format. */
export interface SSEEvent {
  /** Event data string (after the `data:` prefix). */
  data: string
  /** Optional event ID from the `id:` field. */
  id?: string
}

function createSSEDecoder(): TransformStream<string, SSEEvent> {
  let buffer = ""
  let data = ""
  let id: string | undefined

  return new TransformStream<string, SSEEvent>({
    transform(chunk, controller) {
      buffer += chunk
      const lines = buffer.split("\n")
      buffer = lines.pop()!

      for (const line of lines) {
        if (line === "") {
          if (data !== "") {
            controller.enqueue({ data, id })
          }
          data = ""
          id = undefined
        } else if (line.startsWith("data:")) {
          data += (data ? "\n" : "") + line.slice(5)
        } else if (line.startsWith("id:")) {
          id = line.slice(3).trim()
        }
      }
    },
    flush(controller) {
      if (data !== "") {
        controller.enqueue({ data, id })
      }
    },
  })
}

/** Options for [`connectSSE`]. */
export interface SSEConnectOptions {
  /** SSE endpoint URL. */
  url: string
  /** Optional signal to abort the connection. */
  signal?: AbortSignal
  /** Last event ID for reconnection (sent as `Last-Event-ID` header). */
  lastEventId?: string
  /** Request body for POST subscriptions. */
  body?: string
  /** HTTP method — `"GET"` (default) or `"POST"`. */
  method?: "GET" | "POST"
}

/** Result of an SSE connection. */
export interface SSEResult {
  /** Async iterator over parsed SSE events. */
  iterable: AsyncIterable<SSEEvent>
  /** Close the connection and clean up resources. */
  close: () => void
}

/**
 * Connect to an SSE endpoint using `fetch` with a ReadableStream.
 *
 * Returns an `AsyncIterable` of parsed SSE events and a `close` function.
 * Supports automatic reconnection via `Last-Event-ID` and signal-based abort.
 */
export async function connectSSE(opts: SSEConnectOptions): Promise<SSEResult> {
  const headers: Record<string, string> = {
    Accept: "text/event-stream",
  }
  if (opts.lastEventId) {
    headers["Last-Event-ID"] = opts.lastEventId
  }

  const fetchOpts: RequestInit = { headers, signal: opts.signal }
  if (opts.body) {
    fetchOpts.method = opts.method || "POST"
    fetchOpts.body = opts.body
    headers["Content-Type"] = "application/json"
  }

  const response = await fetch(opts.url, fetchOpts)

  if (!response.ok) {
    throw new Error(`SSE connection failed: ${response.status}`)
  }

  const reader = response.body!
    .pipeThrough(new TextDecoderStream())
    .pipeThrough(createSSEDecoder())
    .getReader()

  if (opts.signal) {
    if (opts.signal.aborted) {
      reader.cancel()
    } else {
      opts.signal.addEventListener("abort", () => { reader.cancel() }, { once: true })
    }
  }

  return {
    iterable: {
      [Symbol.asyncIterator]() {
        return {
          next: () => reader.read(),
          return: () => {
            reader.cancel()
            return Promise.resolve({ done: true, value: undefined as any })
          },
        }
      },
    } satisfies AsyncIterable<SSEEvent>,
    close: () => reader.cancel(),
  }
}
