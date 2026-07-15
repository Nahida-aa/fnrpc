export const BIGINT = 0 as const;

type MetaItem = [number, ...(string | number)[]];

export interface Serialized {
  json: unknown;
  meta: MetaItem[];
}

// ── Serialize ────────────────────────────────────────────

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

// ── Flatten meta into plain JSON for Rust (no meta support) ──
// Converts BIGINT string values back to numbers for serde_json compat.

export function flattenForRust(serialized: Serialized): unknown {
  const { json, meta } = serialized;
  if (!meta || meta.length === 0) return json;

  const result = structuredClone(json);

  for (const item of meta) {
    const [typeId, ...segments] = item;

    if (segments.length === 0) {
      switch (typeId) {
        case BIGINT:
          return Number(result as string);
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
        // String → Number. Loses precision > 2^53, but Rust's serde_json
        // expects a JSON number for u64/i64 fields.
        current[lastSeg] = Number(raw as string);
        break;
    }
  }

  return result;
}

// ── safeStringify (for places that need a direct JSON string) ──

export function safeStringify(val: unknown): string {
  return JSON.stringify(val, (_, v) =>
    typeof v === "bigint" ? Number(v) : v,
  );
}
