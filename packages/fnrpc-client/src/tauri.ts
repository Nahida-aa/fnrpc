import { observable } from "./UntypedClient";
import type { ExecuteArgs, ExeceuteData, ExecuteFn } from "./types";

export function tauriExecute(): ExecuteFn {
  return (args: ExecuteArgs) => {
    if (args.type === "subscription") {
      return observable<ExeceuteData>((subscriber) => {
        let canceled = false;

        import("@tauri-apps/api/core")
          .then(({ invoke, Channel }) => {
            const channel = new Channel<string>();
            channel.onmessage = (msg) => {
              if (canceled) return;
              if (msg.startsWith("__error:")) {
                subscriber.next({ code: 500, value: msg.slice(8) });
                subscriber.complete();
              } else {
                subscriber.next({ code: 200, value: msg });
              }
            };

            invoke("rpc_subscribe", {
              path: args.path,
              input: args.input ?? null,
              channel,
            }).catch((err) => {
              if (canceled) return;
              subscriber.next({ code: 500, value: String(err) });
              subscriber.complete();
            });
          })
          .catch((err) => {
            subscriber.next({ code: 500, value: String(err) });
            subscriber.complete();
          });

        return () => {
          canceled = true;
        };
      });
    }

    return observable<ExeceuteData>((subscriber) => {
      import("@tauri-apps/api/core")
        .then(({ invoke }) =>
          invoke("rpc_fn", {
            path: args.path,
            input: args.input ?? null,
          }),
        )
        .then((value) => subscriber.next({ code: 200, value }))
        .catch((err) => {
          console.error("[fnrpc] tauri invoke error:", err);
          subscriber.next({ code: 500, value: String(err) });
        })
        .finally(() => subscriber.complete());
    });
  };
}
