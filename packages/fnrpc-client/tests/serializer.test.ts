import { describe, it, expect } from "bun:test";
import { toRustJson, flattenForRust, serialize, deserialize, isEnvelope, type Serialized } from "../src/serializer";

describe("toRustJson (wire format sent to the Rust backend)", () => {
  it("keeps bigint as a string without precision loss and drops meta", () => {
    const input = {
      id: 18446744073709551615n, // > 2^53, would lose precision as a JS number
      nested: { count: 9007199254740993n },
      list: [5n, 6n],
      plain: "hello",
    };

    const out = toRustJson(input) as Record<string, unknown>;
    const raw = JSON.stringify(out);

    // No meta envelope — the server decodes by its own schema instead.
    expect(raw).not.toContain('"meta"');
    // BigInt fields are preserved as exact strings.
    expect(out.id).toBe("18446744073709551615");
    expect((out.nested as Record<string, unknown>).count).toBe("9007199254740993");
    expect(out.list).toEqual(["5", "6"]);
    expect(out.plain).toBe("hello");
  });

  it("does not narrow a top-level bigint to a lossy number", () => {
    const out = toRustJson(18446744073709551615n) as string;
    expect(out).toBe("18446744073709551615");
  });

  it("produces plain JSON that the server can decode by schema", () => {
    // This is exactly the shape the Rust `decode_bigint_by_schema` unit test
    // consumes; round-trips through JSON.stringify like a real HTTP body.
    const out = toRustJson({ id: 18446744073709551615n });
    const wire = JSON.parse(JSON.stringify(out));
    expect(wire.id).toBe("18446744073709551615");
  });
});

describe("flattenForRust (back-compat, now lossless)", () => {
  it("returns the same string-encoded JSON as toRustJson", () => {
    const input = { id: 18446744073709551615n, list: [1n] };
    const serialized = serialize(input);
    expect(flattenForRust(serialized)).toEqual(toRustJson(input));
  });

  it("no longer narrows bigint to a JS number", () => {
    const serialized = serialize(18446744073709551615n);
    const out = flattenForRust(serialized) as string;
    expect(out).toBe("18446744073709551615");
  });
});

describe("deserialize (response envelope from the Rust server)", () => {
  it("restores BigInt values from a { json, meta } envelope", () => {
    // Shape emitted by the Rust server's `encode_bigint_by_schema`.
    const envelope: Serialized = {
      json: {
        id: "18446744073709551615",
        big: "170141183460469231731687303715884105727",
        list: ["1", "18446744073709551615"],
      },
      meta: [
        [0, "id"],
        [0, "big"],
        [0, "list", "*"],
      ],
    };

    const out = deserialize(envelope) as Record<string, unknown>;
    expect(out.id).toBe(18446744073709551615n);
    expect(out.big).toBe(170141183460469231731687303715884105727n);
    expect(out.list).toEqual([1n, 18446744073709551615n]);
  });

  it("isEnvelope detects the envelope but ignores bare JSON", () => {
    expect(isEnvelope({ json: {}, meta: [] })).toBe(true);
    expect(isEnvelope({ id: 1 })).toBe(false);
    expect(isEnvelope(null)).toBe(false);
    expect(isEnvelope([1, 2, 3])).toBe(false);
  });

  it("deserialize with empty meta returns the bare json", () => {
    const out = deserialize({ json: { a: 1 }, meta: [] }) as Record<string, unknown>;
    expect(out).toEqual({ a: 1 });
  });
});
