import type { Client, Procedures } from "@fnrpc/client";

import { ProcedureUtils } from "./procedure-utils";
import type { RouterUtils, RouterUtilsOptions } from "./types";

export function createRouterUtils<T extends Procedures>(
  client: Client<T>,
  _options?: RouterUtilsOptions<T>,
): RouterUtils<T> {
  function buildProxy(path: string[]): any {
    return new Proxy({} as any, {
      get(_target, key) {
        if (typeof key !== "string") return;
        const nextPath = [...path, key];
        return new ProcedureUtils(nextPath.join("."), client);
      },
    });
  }

  return buildProxy([]);
}
