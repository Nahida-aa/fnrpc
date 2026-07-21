/**
 * BigInt type ID — used in the `meta` array to tag BigInt-encoded paths.
 * Must match `BIGINT_TYPE_ID` in the Rust `serializer.rs`.
 */
export const BIGINT = 0 as const;

type MetaItem = [number, ...(string | number)[]];

/**
 * Serialised value with optional metadata envelopes.
 *
 * - `json`: the JSON-compatible value (BigInts converted to strings).
 * - `meta`: an array of path + type-annotations for reconstructing
 *   JS-specific types on the receiving end.
 */
export interface Serialized {
  json: unknown;
  meta: MetaItem[];
}

// ── Serialize ────────────────────────────────────────────

/**
 * Serialise a JavaScript value for transport, preserving JS-specific types
 * (like `BigInt`) via a metadata envelope.
 *
 * BigInt values are converted to strings in the JSON payload and annotated
 * in the `meta` array for server-side reconstruction.
 */
export function serialize(val: unknown): Serialized {
  const meta: MetaItem[] = [];
  const json = walk(val, [], meta);
  return { json, meta };
}

function walk(
  val: unknown,
  path: (string | number)[],
  meta: MetaItem[],
): unknown {
  if (val === undefined) return null;
  if (typeof val === "bigint") {
    meta.push([BIGINT, ...path]);
    return val.toString();
  }

  if (Array.isArray(val)) {
    return val.map((v, i) => walk(v, [...path, i], meta));
  }

  if (val !== null && typeof val === "object") {
    const obj: Record<string, unknown> = {};
    for (const k of Object.keys(val as Record<string, unknown>)) {
      obj[k] = walk((val as Record<string, unknown>)[k], [...path, k], meta);
    }
    return obj;
  }

  return val;
}

// ── Deserialize ──────────────────────────────────────────

/**
 * Deserialise a `Serialized` value back to its original JS form,
 * restoring BigInt strings to actual `BigInt` values.
 */
export function deserialize(input: Serialized): unknown {
  const { json, meta } = input;
  if (!meta || meta.length === 0) return json;

  const result = structuredClone(json);

  for (const item of meta) {
    const [typeId, ...segments] = item;

    if (segments.length === 0) {
      switch (typeId) {
        case BIGINT:
          return BigInt(result as string);
      }
      continue;
    }

    let current: any = result;

    for (let i = 0; i < segments.length - 1; i++) {
      if (current == null) break;
      current = current[segments[i]];
    }

    if (current == null) continue;

    const lastSeg = segments[segments.length - 1];
    const raw = current[lastSeg];

    switch (typeId) {
      case BIGINT:
        current[lastSeg] = BigInt(raw as string);
        break;
    }
  }

  return result;
}

// ── Serialize for the Rust backend (no precision loss) ──

/**
 * Serialize a value for the fnrpc Rust backend without precision loss.
 *
 * BigInt values are kept as JSON strings (the server converts them back to
 * numbers using its own schema via `fnrpc::serializer::decode_bigint_by_schema`),
 * and the `meta` envelope is dropped — the server does not need it.
 *
 * Unlike the old `flattenForRust` (which narrowed BigInts to JS `number` and
 * lost precision above 2^53), this preserves the full u64/i64 range.
 */
export function toRustJson(val: unknown): unknown {
  return serialize(val).json;
}

/**
 * @deprecated Use {@link toRustJson} instead.
 *
 * Historically flattened BigInts into JS `number`s (precision loss above 2^53).
 * It now returns the same lossless, string-encoded JSON as {@link toRustJson}
 * so existing callers keep working without precision loss.
 */
export function flattenForRust(serialized: Serialized): unknown {
  return serialized.json;
}

// ── safeStringify (for places that need a direct JSON string) ──

/**
 * JSON.stringify with BigInt-to-number conversion.
 * Safer than the default `JSON.stringify` which throws on BigInt values.
 */
export function safeStringify(val: unknown): string {
  return JSON.stringify(val, (_, v) =>
    typeof v === "bigint" ? Number(v) : v,
  );
}
