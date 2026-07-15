export class RpcError extends Error {
  override name = "RpcError" as const;
  code: string;
  data?: unknown;

  constructor(code: string, message: string, data?: unknown) {
    super(message);
    this.code = code;
    this.data = data;
  }

  static fromJson(json: { code: string; message: string; data?: unknown }): RpcError {
    return new RpcError(json.code, json.message, json.data);
  }

  static internal(message: string, data?: unknown): RpcError {
    return new RpcError("INTERNAL_SERVER_ERROR", message, data);
  }

  static badRequest(message: string, data?: unknown): RpcError {
    return new RpcError("BAD_REQUEST", message, data);
  }

  static notFound(message: string, data?: unknown): RpcError {
    return new RpcError("NOT_FOUND", message, data);
  }

  toJSON(): { code: string; message: string; data?: unknown } {
    return { code: this.code, message: this.message, data: this.data };
  }
}

export function isRpcError(err: unknown): err is RpcError {
  return err instanceof RpcError;
}

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
