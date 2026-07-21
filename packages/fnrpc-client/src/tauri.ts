import type { ProcedureKind } from "./types";
import { toRustJson, deserialize, isEnvelope } from "./serializer";
import { RpcError } from "./error";

/**
 * Minimal interface for the Tauri IPC module.
 *
 * Only the parts used by this transport are declared here.
 */
export interface TauriCore {
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
  Channel: new <T = unknown>() => { id: number; onmessage: ((msg: T) => void) | null };
}

function parseError(msg: string): RpcError {
  try {
    const parsed = JSON.parse(msg);
    return RpcError.fromJson(parsed);
  } catch {
    return new RpcError("INTERNAL_SERVER_ERROR", msg);
  }
}

/**
 * Create a transport that communicates via Tauri IPC (`invoke` + `Channel`).
 *
 * The factory takes a lazily-imported `@tauri-apps/api/core` module,
 * which is resolved at runtime. This avoids bundling Tauri dependencies
 * in non-Tauri environments.
 *
 * @example
 * ```typescript
 * const transport = tauriTransport(() => import("@tauri-apps/api/core"));
 * ```
 */
export function tauriTransport(getCore: () => Promise<TauriCore>) {
  return (
    path: string,
    input: unknown,
    kind: ProcedureKind,
    signal?: AbortSignal,
  ): Promise<unknown> => {
    if (kind === "subscribe") {
      return getCore().then(async (mod) => {
        const channel = new mod.Channel<string>();

        let done = false;
        const pending: Array<IteratorResult<unknown>> = [];
        let resolveNext: ((r: IteratorResult<unknown>) => void) | null = null;
        let rejectNext: ((err: unknown) => void) | null = null;

        channel.onmessage = (msg: string) => {
          if (done) return;
          if (msg.startsWith("__error:")) {
            const err = parseError(msg.slice(8));
            done = true;
            if (rejectNext) {
              rejectNext(err);
              rejectNext = null;
            }
          } else {
            let val: unknown;
            try {
              val = JSON.parse(msg);
            } catch {
              val = msg;
            }
            if (isEnvelope(val)) val = deserialize(val);
            if (resolveNext) {
              resolveNext({ done: false, value: val });
              resolveNext = null;
            } else {
              pending.push({ done: false, value: val });
            }
          }
        };

        if (signal) {
          signal.addEventListener(
            "abort",
            () => {
              done = true;
            },
            { once: true },
          );
        }

        const iterable: AsyncIterable<unknown> = {
          [Symbol.asyncIterator]() {
            const channelId = channel.id;
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
                if (resolveNext) {
                  resolveNext({ done: true, value: undefined as any });
                  resolveNext = null;
                }
                // Notify Rust side to cancel the subscription
                mod.invoke("__fnrpc_rpc_cancel_sub", { channel_id: channelId }).catch(() => {});
                return Promise.resolve({ done: true, value: undefined as any });
              },
            };
          },
        } satisfies AsyncIterable<unknown>;

        await mod.invoke("__fnrpc_rpc_sub", {
          path,
          input: toRustJson(input),
          channel,
        });

        return iterable;
      });
    }

    // query / mutate
    return getCore()
      .then((mod) =>
        mod.invoke("__fnrpc_rpc_fn", {
          path,
          input: toRustJson(input),
        }),
      )
      .then((result) => (isEnvelope(result) ? deserialize(result) : result))
      .catch((err: unknown) => {
        const msg = typeof err === "string" ? err : String(err);
        throw parseError(msg);
      });
  };
}
