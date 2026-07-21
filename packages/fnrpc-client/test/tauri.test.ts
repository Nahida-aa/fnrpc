import { tauriTransport } from "../src/tauri";
import type { TauriCore } from "../src/tauri";

function makeCore() {
  const calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  const core: TauriCore = {
    invoke: (cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args: args ?? {} });
      return Promise.resolve(undefined);
    },
    Channel: class {
      id = 1;
      onmessage: ((msg: string) => void) | null = null;
    } as unknown as new <T>() => { id: number; onmessage: ((msg: T) => void) | null },
  };
  return { core, calls };
}

describe("tauriTransport", () => {
  it("calls __fnrpc_rpc_fn for query/mutate", async () => {
    const { core, calls } = makeCore();
    const transport = tauriTransport(() => Promise.resolve(core));

    await transport("get_user", { id: 1 }, "query");

    expect(calls).toHaveLength(1);
    expect(calls[0].cmd).toBe("__fnrpc_rpc_fn");
    expect(calls[0].args.path).toBe("get_user");
    expect(calls[0].args.input).toBeDefined();
  });

  it("calls __fnrpc_rpc_sub for subscribe", async () => {
    const { core, calls } = makeCore();
    const transport = tauriTransport(() => Promise.resolve(core));

    const iterable = (await transport("tick", {}, "subscribe")) as AsyncIterable<unknown>;
    // exhaust the iterator to trigger cancel
    const it = iterable[Symbol.asyncIterator]();

    expect(calls).toHaveLength(1);
    expect(calls[0].cmd).toBe("__fnrpc_rpc_sub");
    expect(calls[0].args.path).toBe("tick");
    expect(calls[0].args.channel).toBeDefined();

    await it.return?.();
    // cancel invokes __fnrpc_rpc_cancel_sub with the channel id
    const cancelCall = calls.find((c) => c.cmd === "__fnrpc_rpc_cancel_sub");
    expect(cancelCall).toBeDefined();
    expect(cancelCall!.args.channel_id).toBe(1);
  });

  it("does NOT call the legacy unprefixed command names", async () => {
    const { core, calls } = makeCore();
    const transport = tauriTransport(() => Promise.resolve(core));

    await transport("get_user", { id: 1 }, "query");

    const cmds = calls.map((c) => c.cmd);
    expect(cmds).not.toContain("rpc_fn");
    expect(cmds).not.toContain("rpc_sub");
    expect(cmds).not.toContain("rpc_cancel_sub");
  });
});
