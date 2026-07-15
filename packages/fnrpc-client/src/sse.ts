export interface SSEEvent {
  data: string
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

export interface SSEConnectOptions {
  url: string
  signal?: AbortSignal
  lastEventId?: string
}

export interface SSEResult {
  iterable: AsyncIterable<SSEEvent>
  close: () => void
}

export async function connectSSE(opts: SSEConnectOptions): Promise<SSEResult> {
  const headers: Record<string, string> = {
    Accept: "text/event-stream",
  }
  if (opts.lastEventId) {
    headers["Last-Event-ID"] = opts.lastEventId
  }

  const response = await fetch(opts.url, { headers, signal: opts.signal })

  if (!response.ok) {
    throw new Error(`SSE connection failed: ${response.status}`)
  }

  const reader = response.body!
    .pipeThrough(new TextDecoderStream())
    .pipeThrough(createSSEDecoder())
    .getReader()

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
