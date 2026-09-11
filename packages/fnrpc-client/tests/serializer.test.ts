import { describe, it, expect } from "bun:test";
import { toRustJson, flattenForRust, serialize, deserialize, type Serialized } from "../src/serializer";

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

  it("deserialize with empty meta returns the bare json", () => {
    const out = deserialize({ json: { a: 1 }, meta: [] }) as Record<string, unknown>;
    expect(out).toEqual({ a: 1 });
  });

  it("skips meta paths that are absent from the payload", () => {
    // `meta` is schema-driven, so it lists every BigInt leaf the *type* can
    // hold — an enum's inactive variants are the common case. Those paths are
    // legitimately missing here and must not be written as `undefined`.
    const out = deserialize({
      json: { Small: "18446744073709551615", tag: "t" },
      meta: [
        [0, "Small"],
        [0, "Named", "big"],
      ],
    }) as Record<string, unknown>;

    expect(out.Small).toBe(18446744073709551615n);
    expect(Object.keys(out)).toEqual(["Small", "tag"]);
    expect("Named" in out).toBe(false);
  });

  it("does not fabricate a key for an absent single-segment path", () => {
    // Regression: the single-segment branch assigned unconditionally, so an
    // unresolvable path added a phantom `undefined` key to the decoded object
    // (silently, with no way for the caller to notice).
    const out = deserialize({
      json: { tag: "t" },
      meta: [[0, "missing"]],
    }) as Record<string, unknown>;

    expect(Object.keys(out)).toEqual(["tag"]);
    expect("missing" in out).toBe(false);
  });

  it("deserialize throws on a non-envelope payload (protocol violation)", () => {
    // The server must always send { json, meta }; bare JSON is a bug, not a
    // tolerated legacy shape — there are no users to stay compatible with.
    expect(() => deserialize({ id: 1 })).toThrow();
    expect(() => deserialize(null)).toThrow();
    expect(() => deserialize([1, 2, 3])).toThrow();
    expect(() => deserialize("hello")).toThrow();
    // `meta` missing / not an array is also a violation.
    expect(() => deserialize({ json: { a: 1 } } as unknown)).toThrow();
  });
});
