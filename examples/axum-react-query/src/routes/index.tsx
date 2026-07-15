import { createFileRoute } from "@tanstack/react-router";
import { useQuery, useMutation } from "@tanstack/react-query";
import { client, fnrpc } from "#/integrations/fnrpc/client.ts";
import { useEffect, useState } from "react";
import { consumeEventIterator } from "@fnrpc/client";

export const Route = createFileRoute("/")({
  component: IndexPage,
});

function IndexPage() {
  return (
    <div style={{ maxWidth: 800, margin: "0 auto", padding: 24, fontFamily: "sans-serif" }}>
      <h1>fnrpc — Axum + React + TanStack Query</h1>
      <p>Axum server + React frontend + TanStack Router + @fnrpc/client</p>
      <hr />
      {/* <QuerySection /> */}
      <hr />
      <MutationSection />
      <hr />
      <SubscriptionSection />
    </div>
  );
}

function QuerySection() {
  const greet = useQuery(client.greet.queryOptions("World"));
  const add = useQuery(client.add.queryOptions([3, 5]));
  const get_user = useQuery(client.get_user.queryOptions(1));
  const divide = useQuery(client.divide.queryOptions([10, 2]));

  return (
    <section>
      <h2>Queries</h2>
      <table style={{ width: "100%", borderCollapse: "collapse" }}>
        <thead>
          <tr>
            <th style={{ textAlign: "left", borderBottom: "1px solid #ccc" }}>Procedure</th>
            <th style={{ textAlign: "left", borderBottom: "1px solid #ccc" }}>Input</th>
            <th style={{ textAlign: "left", borderBottom: "1px solid #ccc" }}>Output</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td style={{ borderBottom: "1px solid #eee" }}>greet</td>
            <td style={{ borderBottom: "1px solid #eee" }}>"World"</td>
            <td style={{ borderBottom: "1px solid #eee" }}>{greet.data ?? "loading..."}</td>
          </tr>
          <tr>
            <td style={{ borderBottom: "1px solid #eee" }}>add</td>
            <td style={{ borderBottom: "1px solid #eee" }}>[3, 5]</td>
            <td style={{ borderBottom: "1px solid #eee" }}>{add.data ?? "loading..."}</td>
          </tr>
          <tr>
            <td style={{ borderBottom: "1px solid #eee" }}>get_user</td>
            <td style={{ borderBottom: "1px solid #eee" }}>1</td>
            <td style={{ borderBottom: "1px solid #eee" }}>{get_user.data ? JSON.stringify(get_user.data) : "loading..."}</td>
          </tr>
          <tr>
            <td style={{ borderBottom: "1px solid #eee" }}>divide</td>
            <td style={{ borderBottom: "1px solid #eee" }}>[10, 2]</td>
            <td style={{ borderBottom: "1px solid #eee" }}>{divide.data ?? "loading..."}</td>
          </tr>
        </tbody>
      </table>
    </section>
  );
}

function MutationSection() {
  const create_user = useMutation(client.create_user.mutationOptions());

  return (
    <section>
      <h2>Mutations</h2>
      <button
        onClick={() => create_user.mutate(["Bob", "bob@example.com"] )}
        disabled={create_user.isPending}
      >
        {create_user.isPending ? "Creating..." : "Create User"}
      </button>
      {create_user.data && (
        <pre style={{ marginTop: 8, fontSize: 12 }}>{JSON.stringify(create_user.data, null, 2)}</pre>
      )}
    </section>
  );
}

function SubscriptionSection() {
  return (
    <section>
      <h2>Subscriptions</h2>
      <EchoTest />
      <EchoWithConsumeEventIterator />
      <LiveSubscriptionDemo />
      <hr />
      <StreamedSubscriptionDemo />
      <hr />
      <PostSubscriptionDemo />
    </section>
  );
}

function Row(props: { label: string; children: any }) {
  return (
    <div className="flex items-center gap-2 flex-wrap min-h-9 px-3 py-2 rounded-lg bg-card border text-sm">
      <span className="font-mono text-xs text-muted-foreground shrink-0 w-36">{props.label}</span>
      {props.children}
    </div>
  );
}

function EchoTest() {
  const [prefix, setPrefix] = useState('msg');
  const [msgs, setMsgs] = useState<string[]>([]);
  const [running, setRunning] = useState(false);
  useEffect(() => {
    if (!running) return;
    const ac = new AbortController();
    let cancelled = false;

    (async () => {
      try {
        const iter = await fnrpc.echo_stream(prefix, ac.signal);
        for await (const v of iter) {
          if (cancelled) break;
          setMsgs(prev => [...prev, String(v)]);
        }
      } catch (e) {
        console.error('echo error', e);
      }
    })();
    return () => {
      cancelled = true;
      ac.abort();
    }
  }, [running, prefix]);
  return <Row label="echo_stream(prefix)">
      <input className="border rounded px-2 py-0.5 bg-background text-sm w-24" value={prefix} onInput={e => setPrefix(e.currentTarget.value)} />
      <button
        className={running
          ? 'bg-red-600 text-white px-3 py-1 rounded text-sm hover:bg-red-700'
          : 'bg-green-600 text-white px-3 py-1 rounded text-sm hover:bg-green-700'}
        onClick={() => setRunning(!running)}
      >
        {running ? 'Stop' : 'Start'}
      </button>
      <div className="text-xs text-muted-foreground truncate max-w-60">
        {msgs.join(', ')}
      </div>
    </Row>
}
function EchoWithConsumeEventIterator() {
  const [prefix, setPrefix] = useState('msg');
  const [msgs, setMsgs] = useState<string[]>([]);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    if (!running) { setMsgs([]); return; }
    const iter = fnrpc.echo_stream(prefix);
    const cancel = consumeEventIterator(iter, {
      onEvent: v => setMsgs(prev => [...prev, String(v)]),
    });
    return () => cancel()
  }, [running, prefix]);

  return (
    <Row label="echo_stream (consumeEventIterator)">
      <input className="border rounded px-2 py-0.5 bg-background text-sm w-24" value={prefix} onInput={e => setPrefix(e.currentTarget.value)} />
      <button
        className={running
          ? 'bg-red-600 text-white px-3 py-1 rounded text-sm hover:bg-red-700'
          : 'bg-green-600 text-white px-3 py-1 rounded text-sm hover:bg-green-700'}
        onClick={() => setRunning(!running)}
      >
        {running ? 'Stop' : 'Start'}
      </button>
      <div className="text-xs text-muted-foreground truncate max-w-60">
        {msgs.join(', ')}
      </div>
    </Row>
  );
}
function LiveSubscriptionDemo() {
  const [running, setRunning] = useState(false);
  const query = useQuery(
    client.tick.liveOptions(500n, { enabled: running }),
  );

  return (
    <div>
      <h3>Live (tick)</h3>
      <button onClick={() => setRunning(!running)}>
        {running ? "Stop" : "Start"}
      </button>
      <span style={{ marginLeft: 8 }}>{query.data ?? "—"}</span>
    </div>
  );
}

function StreamedSubscriptionDemo() {
  const [running, setRunning] = useState(false);
  const query = useQuery(
    client.echo_stream.streamedOptions("hello", { enabled: running }),
  );

  return (
    <div>
      <h3>Streamed (echo_stream)</h3>
      <button onClick={() => setRunning(!running)}>
        {running ? "Stop" : "Start"}
      </button>
      <pre style={{ fontSize: 12, maxHeight: 120, overflow: "auto" }}>
        {query.data ? JSON.stringify(query.data) : "—"}
      </pre>
    </div>
  );
}

function PostSubscriptionDemo() {
  const [running, setRunning] = useState(false);
  const query = useQuery(
    client.post_echo_stream.liveOptions("post_msg", { enabled: running }),
  );

  return (
    <div>
      <h3>POST Live (post_echo_stream)</h3>
      <button onClick={() => setRunning(!running)}>
        {running ? "Stop" : "Start"}
      </button>
      <span style={{ marginLeft: 8 }}>{query.data ?? "—"}</span>
    </div>
  );
}
