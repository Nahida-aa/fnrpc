import { describe, it, expect } from "bun:test";
import { toRustJson, flattenForRust, serialize } from "../src/serializer";

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
