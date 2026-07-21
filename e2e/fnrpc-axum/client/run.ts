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
    async function check(name: string, expected: string, actual: string) {
      if (actual !== expected) {
        throw new Error(
          `[${name}] precision mismatch!\n  expected: ${expected}\n  received: ${actual}`,
        );
      }
      console.log(`OK [${name}]: ${actual}`);
      passed++;
    }

    // 2. Query/mutate assertions through the typed client.

    // Original: struct with u64 / i128 / Vec<u64>, GET query.
    await check(
      "big_echo (struct, GET query)",
      "id=18446744073709551615 big=170141183460469231731687303715884105727 list=[1, 18446744073709551615]",
      await fnrpc.big_echo(INPUT),
    );

    // Top-level primitive u64, GET query.
    await check(
      "big_echo_primitive (u64, GET query)",
      "input=18446744073709551615",
      await fnrpc.big_echo_primitive(18446744073709551615n),
    );

    // Top-level primitive u64, query forced to POST.
    await check(
      "big_echo_primitive_post (u64, POST query)",
      "input=18446744073709551615",
      await fnrpc.big_echo_primitive_post(18446744073709551615n),
    );

    // Top-level primitive u64, mutate (POST).
    await check(
      "big_echo_primitive_mutate (u64, POST mutate)",
      "input=18446744073709551615",
      await fnrpc.big_echo_primitive_mutate(18446744073709551615n),
    );

    // Struct with u64 / i128 / Vec<u64>, mutate (POST).
    await check(
      "big_echo_mutate (struct, POST mutate)",
      "id=18446744073709551615 big=170141183460469231731687303715884105727 list=[1, 18446744073709551615]",
      await fnrpc.big_echo_mutate(INPUT),
    );

    // 3. SSE subscription assertion: BigInt precision + subscribe transport.
    // The SSE client auto-reconnects, so we collect the expected messages and
    // then abort to end the (long-lived) stream.
    const controller = new AbortController();
    const iter = await fnrpc.tick_seq(
      { start: 18446744073709551615n, count: 3n },
      controller.signal,
    );
    const msgs: string[] = [];
    for await (const m of iter) {
      msgs.push(m as string);
      if (msgs.length >= 4) {
        controller.abort();
        break;
      }
    }

    if (msgs[0] !== "start=18446744073709551615") {
      throw new Error(
        `[tick_seq] start precision mismatch!\n  expected: start=18446744073709551615\n  received: ${msgs[0]}`,
      );
    }
    if (msgs.slice(1).join(",") !== "n=0,n=1,n=2") {
      throw new Error(
        `[tick_seq] tick content mismatch!\n  expected: n=0,n=1,n=2\n  received: ${msgs.slice(1).join(",")}`,
      );
    }
    console.log(`OK [tick_seq (SSE subscribe)]: ${msgs.join(" | ")}`);
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
