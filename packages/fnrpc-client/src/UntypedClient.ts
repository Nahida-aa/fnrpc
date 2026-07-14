import type {
  ExeceuteData,
  ExecuteArgs,
  ExecuteFn,
  SubscriptionObserver,
  Unsubscribable,
} from "./types";

export function observable<T>(
  cb: (
    subscriber: {
      next: (value: T) => void;
      error: (err: unknown) => void;
      complete(): void;
    },
  ) => (() => void) | void,
) {
  let callbacks: Array<(v: T) => void> = [];
  let completeCallbacks: Array<() => void> = [];
  let done = false;
  let cleanup: (() => void) | null = null;

  cleanup =
    cb({
      next: (v) => {
        if (done) return;
        callbacks.forEach((cb) => cb(v));
      },
      error: (err) => {
        if (done) return;
        done = true;
        errorCallbacks.forEach((cb) => cb(err));
        completeCallbacks.forEach((cb) => cb());
      },
      complete: () => {
        if (done) return;
        done = true;
        completeCallbacks.forEach((cb) => cb());
      },
    }) ?? null;

  let errorCallbacks: Array<(err: unknown) => void> = [];

  const result = {
    subscribe(cb: (v: T) => void) {
      if (done) return Promise.resolve();
      callbacks.push(cb);
      return new Promise<void>((res) => {
        completeCallbacks.push(() => res());
      });
    },
    onError(cb: (err: unknown) => void) {
      if (done) return;
      errorCallbacks.push(cb);
    },
    get done() {
      return done;
    },
    unsubscribe() {
      done = true;
      cleanup?.();
      callbacks = [];
      errorCallbacks = [];
      completeCallbacks = [];
    },
  };

  return result;
}

export type Observable<T> = ReturnType<typeof observable<T>>;

// JSON.stringify throws on BigInt values. This replacer converts BigInt to
// Number so JSON transport works. Values > 2^53 lose precision — acceptable
// for this transport (JSON has no native bigint type; the Rust backend
// receives a JSON number either way).
function safeStringify(value: unknown): string {
  return JSON.stringify(value, (_, val) =>
    typeof val === "bigint" ? Number(val) : val,
  );
}

export const fetchExecute = (
  config: { url: string },
  args: ExecuteArgs,
): ReturnType<ExecuteFn> => {
  if (args.type === "subscription") {
    return observable<ExeceuteData>((subscriber) => {
      const params = new URLSearchParams({
        input: safeStringify(args.input),
      });
      const es = new EventSource(
        `${config.url}/sub/${args.path}?${params}`,
      );

      es.onmessage = (e) => {
        try {
          subscriber.next({ code: 200, value: JSON.parse(e.data) });
        } catch {
          subscriber.next({ code: 500, value: e.data });
        }
      };

      es.onerror = () => {
        subscriber.complete();
      };

      return () => es.close();
    });
  }

  let promise: Promise<Response>;
  if (args.type === "query") {
    promise = fetch(
      `${config.url}/${args.path}?${new URLSearchParams({
        input: safeStringify(args.input),
      })}`,
      {
        method: "GET",
        headers: {
          Accept: "application/json",
        },
      },
    );
  } else {
    promise = fetch(`${config.url}/${args.path}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
      body: safeStringify(args.input),
    });
  }

  return observable<ExeceuteData>((subscriber) => {
    promise
      .then(async (r) => {
        if (r.status === 200) {
          subscriber.next({ code: 200, value: await r.json() });
        } else {
          const err = await r.json();
          subscriber.next({ code: r.status, value: err });
        }
      })
      .finally(() => subscriber.complete());
  });
};

export class UntypedClient {
  constructor(public execute: ExecuteFn) {}

  private async executeAsPromise(args: ExecuteArgs) {
    const obs = this.execute(args);

    let data: ExeceuteData | undefined;

    await obs.subscribe((d) => {
      if (data === undefined) data = d;
    });

    if (!data) throw new Error("No data received");
    if (data.code !== 200)
      throw new Error(
        `Error with code '${data.code}' occurred`,
        data.value,
      );

    return data.value;
  }

  public query(path: string, input: unknown) {
    return this.executeAsPromise({ type: "query", path, input });
  }
  public mutation(path: string, input: unknown) {
    return this.executeAsPromise({ type: "mutation", path, input });
  }
  public subscription(
    path: string,
    input: unknown,
    opts?: Partial<SubscriptionObserver<unknown, unknown>>,
  ): Unsubscribable {
    const obs = this.execute({ type: "subscription", path, input });

    obs.subscribe((data) => {
      if (data && data.code === 200) {
        opts?.onData?.(data.value);
      } else {
        opts?.onError?.(data?.value ?? new Error("Unknown error"));
      }
    });

    obs.onError((err) => {
      opts?.onError?.(err);
    });

    return {
      unsubscribe: () => obs.unsubscribe(),
    };
  }
}
