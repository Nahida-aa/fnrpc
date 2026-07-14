import { fnrpc, client } from '#/integrations/fnrpc/client.ts';
import { CreateQueryResult } from '@tanstack/solid-query';
import { createFileRoute } from '@tanstack/solid-router';
import { createSignal, createEffect, onCleanup, Show } from 'solid-js';

export const Route = createFileRoute('/')({
  component: IndexPage,
});

function IndexPage() {
  return (
    <div class="p-6 max-w-3xl mx-auto space-y-10">
      <h1 class="text-2xl font-bold">fnrpc Examples</h1>
      <QuerySection />
      <MutationSection />
      <SubscriptionSection />
    </div>
  );
}

function QuerySection() {
  const health =  fnrpc.createQuery(() => ['health_check']);
  const [name, setName] = createSignal('World');
  const greet = fnrpc.createQuery(() => ['greet', name()]);
  const [a, setA] = createSignal(1);
  const [b, setB] = createSignal(2);
  const add = fnrpc.createQuery(() => ['add', [a(), b()] as [number, number]]);
  const [uid, setUid] = createSignal(1);
  const user = fnrpc.createQuery(() => ['get_user', uid()]);
  const [x, setX] = createSignal(10);
  const [y, setY] = createSignal(2);
  const divide = fnrpc.createQuery(() => ['divide', [x(), y()] as [number, number]]);

  return (
    <section class="space-y-3">
      <h2 class="text-lg font-semibold border-b pb-1">Queries</h2>

      <Row label="health_check()">
        <QueryResult query={fnrpc.createQuery(() => ['health_check'])} />
      </Row>

      <Row label="greet(name)">
        <input class="border rounded px-2 py-0.5 bg-background text-sm" value={name()} onInput={e => setName(e.currentTarget.value)} />
        <QueryResult query={greet} />
      </Row>

      <Row label="add(a,b)">
        <input class="border rounded px-2 py-0.5 bg-background text-sm w-16" type="number" value={a()} onInput={e => setA(Number(e.currentTarget.value))} />
        <span>+</span>
        <input class="border rounded px-2 py-0.5 bg-background text-sm w-16" type="number" value={b()} onInput={e => setB(Number(e.currentTarget.value))} />
        <span>=</span>
        <QueryResult query={add} />
      </Row>

      <Row label="get_user(id)">
        <input class="border rounded px-2 py-0.5 bg-background text-sm w-20" type="number" value={uid()} onInput={e => setUid(Number(e.currentTarget.value))} />
        <QueryResult query={user} />
      </Row>

      <Row label="divide(a,b)">
        <input class="border rounded px-2 py-0.5 bg-background text-sm w-20" type="number" value={x()} onInput={e => setX(Number(e.currentTarget.value))} />
        <span>/</span>
        <input class="border rounded px-2 py-0.5 bg-background text-sm w-20" type="number" value={y()} onInput={e => setY(Number(e.currentTarget.value))} />
        <span>=</span>
        <QueryResult query={divide} />
      </Row>
    </section>
  );
}

function MutationSection() {
  const [name, setName] = createSignal('Bob');
  const [email, setEmail] = createSignal('bob@test.com');
  const mutation = fnrpc.createMutation(() => 'create_user');

  return (
    <section class="space-y-3">
      <h2 class="text-lg font-semibold border-b pb-1">Mutations</h2>

      <Row label="create_user(name,email)">
        <input class="border rounded px-2 py-0.5 bg-background text-sm" value={name()} onInput={e => setName(e.currentTarget.value)} placeholder="name" />
        <input class="border rounded px-2 py-0.5 bg-background text-sm" value={email()} onInput={e => setEmail(e.currentTarget.value)} placeholder="email" />
        <button
          class="bg-primary text-primary-foreground px-3 py-1 rounded text-sm font-medium hover:opacity-90 disabled:opacity-50"
          onClick={() => mutation.mutate([name(), email()])}
          disabled={mutation.isPending}
        >
          {mutation.isPending ? 'Saving...' : 'Create'}
        </button>
        <Show when={mutation.data}>
          {data => <span class="ml-2 text-sm">{JSON.stringify(data())}</span>}
        </Show>
        <Show when={mutation.error}>
          {err => <span class="ml-2 text-sm text-destructive">{err().message}</span>}
        </Show>
      </Row>
    </section>
  );
}

function SubscriptionSection() {
  return (
    <section class="space-y-3">
      <h2 class="text-lg font-semibold border-b pb-1">Subscriptions</h2>
      <TickTest />
      <EchoTest />
      <WatchTest />
    </section>
  );
}

function TickTest() {
  const [count, setCount] = createSignal<number | null>(null);
  const [running, setRunning] = createSignal(false);

  createEffect(() => {
    if (!running()) return;
    const sub = client.tick.subscribe(BigInt(500), {
      onData: v => setCount(Number(v)),
    });
    onCleanup(() => sub.unsubscribe());
  });

  return (
    <Row label="tick(ms)">
      <span class="text-muted-foreground text-xs">500ms</span>
      <button
        class={running()
          ? 'bg-red-600 text-white px-3 py-1 rounded text-sm hover:bg-red-700'
          : 'bg-green-600 text-white px-3 py-1 rounded text-sm hover:bg-green-700'}
        onClick={() => setRunning(!running())}
      >
        {running() ? 'Stop' : 'Start'}
      </button>
      <Show when={count() !== null}>
        <span class="font-mono text-sm">Value: {count()}</span>
      </Show>
    </Row>
  );
}

function EchoTest() {
  const [prefix, setPrefix] = createSignal('msg');
  const [msgs, setMsgs] = createSignal<string[]>([]);
  const [running, setRunning] = createSignal(false);

  createEffect(() => {
    if (!running()) { setMsgs([]); return; }
    const sub = client.echo_stream.subscribe(prefix(), {
      onData: v => setMsgs(prev => [...prev, String(v)]),
    });
    onCleanup(() => sub.unsubscribe());
  });

  return (
    <Row label="echo_stream(prefix)">
      <input class="border rounded px-2 py-0.5 bg-background text-sm w-24" value={prefix()} onInput={e => setPrefix(e.currentTarget.value)} />
      <button
        class={running()
          ? 'bg-red-600 text-white px-3 py-1 rounded text-sm hover:bg-red-700'
          : 'bg-green-600 text-white px-3 py-1 rounded text-sm hover:bg-green-700'}
        onClick={() => setRunning(!running())}
      >
        {running() ? 'Stop' : 'Start'}
      </button>
      <div class="text-xs text-muted-foreground truncate max-w-60">
        {msgs().join(', ')}
      </div>
    </Row>
  );
}

function WatchTest() {
  const [key, setKey] = createSignal('demo');
  const [msgs, setMsgs] = createSignal<string[]>([]);
  const [running, setRunning] = createSignal(false);

  createEffect(() => {
    if (!running()) { setMsgs([]); return; }
    const sub = client.watch_status.subscribe(key(), {
      onData: v => setMsgs(prev => [...prev, String(v)]),
    });
    onCleanup(() => sub.unsubscribe());
  });

  return (
    <Row label="watch_status(key)">
      <input class="border rounded px-2 py-0.5 bg-background text-sm w-24" value={key()} onInput={e => setKey(e.currentTarget.value)} />
      <button
        class={running()
          ? 'bg-red-600 text-white px-3 py-1 rounded text-sm hover:bg-red-700'
          : 'bg-green-600 text-white px-3 py-1 rounded text-sm hover:bg-green-700'}
        onClick={() => setRunning(!running())}
      >
        {running() ? 'Stop' : 'Start'}
      </button>
      <div class="text-xs text-muted-foreground truncate max-w-60">
        {msgs().join(' | ')}
      </div>
    </Row>
  );
}

function Row(props: { label: string; children: any }) {
  return (
    <div class="flex items-center gap-2 flex-wrap min-h-9 px-3 py-2 rounded-lg bg-card border text-sm">
      <span class="font-mono text-xs text-muted-foreground shrink-0 w-36">{props.label}</span>
      {props.children}
    </div>
  );
}

function QueryResult(props: { query: CreateQueryResult }) {
  return (
    <>
      <Show when={props.query.isLoading}>
        <span class="text-muted-foreground text-xs">loading...</span>
      </Show>
      <Show when={props.query.error}>
        {err => <span class="text-destructive text-xs">{err().message}</span>}
      </Show>
      <Show when={props.query.data}>
        {data => <span class="font-mono text-sm">{JSON.stringify(data())}</span>}
      </Show>
    </>
  );
}
