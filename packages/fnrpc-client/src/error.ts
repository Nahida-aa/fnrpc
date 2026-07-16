/**
 * An RPC error from the server.
 *
 * Mirrors the Rust `RpcErr` struct:
 * - `name` is always `"RpcErr"`
 * - `code` is a machine-readable error code (e.g. `"NOT_FOUND"`)
 * - `message` is a human-readable description
 * - `data` holds optional arbitrary JSON payload
 */
export class RpcError extends Error {
  override name = "RpcErr" as const;
  code: string;
  data?: unknown;

  constructor(code: string, message: string, data?: unknown) {
    super(message);
    this.code = code;
    this.data = data;
    Object.defineProperty(this, "message", {
      value: message,
      enumerable: true,
      writable: true,
      configurable: true,
    });
  }

  /** Create an `RpcError` from a JSON object (as returned by the server). */
  static fromJson(json: { name?: string; code: string; message: string; data?: unknown }): RpcError {
    return new RpcError(json.code, json.message, json.data);
  }

  /** Shorthand for a generic internal error. */
  static internal(message: string, data?: unknown): RpcError {
    return new RpcError("INTERNAL_SERVER_ERROR", message, data);
  }

  /** Shorthand for a bad request error. */
  static badRequest(message: string, data?: unknown): RpcError {
    return new RpcError("BAD_REQUEST", message, data);
  }

  /** Shorthand for a not-found error. */
  static notFound(message: string, data?: unknown): RpcError {
    return new RpcError("NOT_FOUND", message, data);
  }

  toJSON(): { name: string; code: string; message: string; data?: unknown } {
    return { name: this.name, code: this.code, message: this.message, data: this.data };
  }
}

/** Type guard for [`RpcError`]. */
export function isRpcError(err: unknown): err is RpcError {
  return err instanceof RpcError;
}

/**
 * Wrap a promise to return a result tuple, converting any error to `RpcError`.
 *
 * @returns `{ ok: true, data }` on success, or `{ ok: false, error }` on failure.
 */
export async function safe<T>(
  promise: Promise<T>,
): Promise<{ ok: true; data: T } | { ok: false; error: RpcError }> {
  try {
    const data = await promise;
    return { ok: true, data };
  } catch (err) {
    if (err instanceof RpcError) {
      return { ok: false, error: err };
    }
    return { ok: false, error: RpcError.internal(String(err)) };
  }
}
