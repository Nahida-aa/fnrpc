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

  // Top-level bigint (whole response is a single bigint string).
  if (meta.length === 1 && meta[0].length === 1) {
    const [typeId] = meta[0];
    if (typeId === BIGINT && typeof json === "string") return BigInt(json);
  }

  const result = structuredClone(json);

  for (const item of meta) {
    const [typeId, ...segments] = item;
    applyMeta(result, segments, typeId);
  }

  return result;
}

/**
 * Walk `segments` (the path recorded by the Rust server) and restore the
 * BigInt at each matching leaf. A `"*"` segment fans out across every array
 * element / object value, mirroring the server's `AnyElem` / `AnyKey`.
 *
 * `segments` is always non-empty here (the empty-segments case is handled by
 * the top-level branch in `deserialize`).
 */
function applyMeta(node: any, segments: (string | number)[], typeId: number): void {
  // Empty path: `node` itself is the bigint leaf (e.g. an array element fanned
  // out by a preceding "*"). Convert it in place where the parent holds it.
  if (segments.length === 0) {
    convertLeaf(node, typeId);
    return;
  }

  const [head, ...rest] = segments;

  if (head === "*") {
    if (Array.isArray(node)) {
      for (let i = 0; i < node.length; i++) {
        node[i] = convertLeaf(node[i], typeId);
      }
    } else if (node != null && typeof node === "object") {
      for (const key of Object.keys(node)) {
        node[key] = convertLeaf(node[key], typeId);
      }
    }
    return;
  }

  if (rest.length === 0) {
    if (node != null && typeof node === "object") {
      node[head] = convertLeaf(node[head], typeId);
    }
    return;
  }

  if (node != null && typeof node === "object" && node[head] != null) {
    applyMeta(node[head], rest, typeId);
  }
}

/// Convert a single value to its JS form per `typeId`. Returns the value
/// unchanged when it isn't the expected raw (string) form.
function convertLeaf(value: any, typeId: number): any {
  switch (typeId) {
    case BIGINT:
      return typeof value === "string" ? BigInt(value) : value;
    default:
      return value;
  }
}

/**
 * Detect whether a parsed JSON response is a BigInt envelope
 * (`{ json, meta }`) produced by the fnrpc server.
 *
 * The server only emits this envelope when the response actually contains
 * BigInt-style integers; everything else is returned as bare JSON.
 */
export function isEnvelope(value: unknown): value is Serialized {
  return (
    typeof value === "object" &&
    value !== null &&
    "json" in value &&
    "meta" in value &&
    Array.isArray((value as Serialized).meta)
  );
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
