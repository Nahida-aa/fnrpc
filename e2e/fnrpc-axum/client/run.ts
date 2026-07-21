/**
 * End-to-end test for the fnrpc-axum example.
 *
 * Steps:
 *   1. Generate TS bindings from the server router
 *      (`cargo run --bin gen_fnrpc` -> `./src/bindings.ts`).
 *   2. Spawn the Rust server (`fnrpc-axum` + `axum`).
 *   3. Call the typed procedures through `createClient` and assert BigInt
 *      precision on the request path (u64 max / i128 max survive as strings,
 *      no `meta` envelope, no precision loss).
 *   4. Subscribe to the SSE stream and assert both BigInt precision and the
 *      subscribe transport end-to-end.
 *
 * The server handlers return `String` confirmations that embed the exact
 * received values, so we can verify precision without depending on
 * response-side BigInt envelope handling (that's a later piece of work).
 *
 * Run:  bun run            (regenerates bindings, spawns server, asserts)
 */

import { spawn, type Subprocess } from "bun";
import { fnrpc } from "./src/client";

const PORT = 3000;
const BASE = `http://localhost:${PORT}`;
const SERVER_MANIFEST = `${import.meta.dir}/../server/Cargo.toml`;
const GEN_BIN = "gen_fnrpc";

// Values that exceed JS's 2^53 safe-integer range — these prove precision is
// preserved end-to-end (a naive `Number()` would corrupt them).
const INPUT = {
  id: 18446744073709551615n, // u64 max
  big: 170141183460469231731687303715884105727n, // i128 max
  list: [1n, 18446744073709551615n],
};

function runGen(): Promise<void> {
  return new Promise((resolve, reject) => {
    const proc = spawn(
      ["cargo", "run", "--bin", GEN_BIN, "--manifest-path", SERVER_MANIFEST],
      { stdout: "inherit", stderr: "inherit" },
    );
    proc.exited.then((code) => {
      if (code === 0) resolve();
      else reject(new Error(`gen_fnrpc exited with code ${code}`));
    });
  });
}

function startServer(): Subprocess {
  const proc = spawn(
    ["cargo", "run", "--bin", "e2e-fnrpc-axum-server", "--manifest-path", SERVER_MANIFEST],
    { stdout: "inherit", stderr: "inherit" },
  );
  return proc;
}

async function waitForServer(timeoutMs = 30000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(BASE);
      // 404 is fine — the server is up, just no handler at "/".
      if (res.status < 500) return;
    } catch {
      // not ready yet
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error("timed out waiting for the fnrpc-axum server to start");
}

async function main() {
  // 1. Regenerate bindings from the current server router.
  await runGen();

  const server = startServer();
  try {
    await waitForServer();

    let passed = 0;
    function assertBig(name: string, actual: unknown, expected: bigint) {
      if (typeof actual !== "bigint" || actual !== expected) {
        throw new Error(
          `[${name}] BigInt precision mismatch!\n  expected: ${expected}\n  received: ${String(actual)} (${typeof actual})`,
        );
      }
      console.log(`OK [${name}]: ${actual}`);
      passed++;
    }
    function assertEq<T>(name: string, actual: T, expected: T) {
      const a = JSON.stringify(actual, (_k, v) => (typeof v === "bigint" ? v.toString() + "n" : v));
      const e = JSON.stringify(expected, (_k, v) => (typeof v === "bigint" ? v.toString() + "n" : v));
      if (a !== e) {
        throw new Error(
          `[${name}] mismatch!\n  expected: ${e}\n  received: ${a}`,
        );
      }
      console.log(`OK [${name}]: ${a}`);
      passed++;
    }

    // 2. Query/mutate assertions through the typed client.
    // These now assert the *response* (server -> client) BigInt envelope:
    // the client receives `{ json, meta }` and restores `BigInt` values.

    // big_echo: struct with u64 / i128 / Vec<u64>, GET query — echoes input.
    // The response output is a BigInput; assert the values survive as BigInt.
    const echo = await fnrpc.big_echo(INPUT);
    assertBig(
      "big_echo (struct, GET query)",
      echo.id,
      18446744073709551615n,
    );
    assertBig(
      "big_echo (struct, GET query)",
      echo.big,
      170141183460469231731687303715884105727n,
    );
    assertEq(
      "big_echo (struct, GET query) list",
      echo.list,
      [1n, 18446744073709551615n],
    );

    // big_echo_mutate: same struct, POST mutate.
    const echoMut = await fnrpc.big_echo_mutate(INPUT);
    assertBig(
      "big_echo_mutate (struct, POST mutate)",
      echoMut.id,
      18446744073709551615n,
    );
    assertBig(
      "big_echo_mutate (struct, POST mutate)",
      echoMut.big,
      170141183460469231731687303715884105727n,
    );
    assertEq(
      "big_echo_mutate (struct, POST mutate) list",
      echoMut.list,
      [1n, 18446744073709551615n],
    );

    // big_out: server returns a bigint struct (u64 max / i128 max).
    const out = await fnrpc.big_out();
    assertBig("big_out (u64 max)", out.id, 18446744073709551615n);
    assertBig(
      "big_out (i128 max)",
      out.big,
      170141183460469231731687303715884105727n,
    );
    assertEq("big_out list", out.list, [1n, 18446744073709551615n]);

    // Top-level primitive u64, GET query (still echoes a String confirmation,
    // so this one is a plain string check — no response bigint envelope).
    const prim = await fnrpc.big_echo_primitive(18446744073709551615n);
    if (prim !== "input=18446744073709551615") {
      throw new Error(
        `[big_echo_primitive] mismatch: expected input=18446744073709551615, got ${prim}`,
      );
    }
    console.log(`OK [big_echo_primitive (u64, GET query)]: ${prim}`);
    passed++;

    const primPost = await fnrpc.big_echo_primitive_post(18446744073709551615n);
    if (primPost !== "input=18446744073709551615") {
      throw new Error(
        `[big_echo_primitive_post] mismatch: expected input=18446744073709551615, got ${primPost}`,
      );
    }
    console.log(`OK [big_echo_primitive_post (u64, POST query)]: ${primPost}`);
    passed++;

    const primMut = await fnrpc.big_echo_primitive_mutate(18446744073709551615n);
    if (primMut !== "input=18446744073709551615") {
      throw new Error(
        `[big_echo_primitive_mutate] mismatch: expected input=18446744073709551615, got ${primMut}`,
      );
    }
    console.log(`OK [big_echo_primitive_mutate (u64, POST mutate)]: ${primMut}`);
    passed++;

    // 3. SSE subscription assertion: response-direction BigInt envelope over
    // SSE. Each emitted `TickOutput.n` is a `u64`, restored to BigInt by the
    // client. The SSE client auto-reconnects, so we collect the expected
    // messages and then abort to end the (long-lived) stream.
    const controller = new AbortController();
    const iter = await fnrpc.tick_seq(
      { start: 18446744073709551615n, count: 3n },
      controller.signal,
    );
    const ns: bigint[] = [];
    for await (const m of iter) {
      ns.push(m.n as bigint);
      if (ns.length >= 4) {
        controller.abort();
        break;
      }
    }

    // First tick carries the request's u64 max (proves request precision),
    // then n=0,1,2 (proves response precision over SSE).
    assertEq("tick_seq (SSE subscribe) sequence", ns, [
      18446744073709551615n,
      0n,
      1n,
      2n,
    ]);
    console.log(
      `OK [tick_seq (SSE subscribe)]: ${ns.map((n) => n.toString()).join(" | ")}`,
    );
    passed++;

    console.log(
      `ALL OK (${passed} checks): server decoded BigInt requests at full precision (no meta, no precision loss), SSE subscribe works`,
    );
  } finally {
    server.kill();
  }
}

main().catch((err) => {
  console.error("e2e FAILED:", err);
  process.exit(1);
});
