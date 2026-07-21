export * from "./types";
export { fetchTransport, consumeEventIterator } from "./UntypedClient";
export { tauriTransport } from "./tauri";
export { serialize, deserialize, flattenForRust, toRustJson, safeStringify, BIGINT } from "./serializer";
export {
  createClient,
  createProceduresProxy,
  getQueryKey,
  traverseClient,
} from "./createClient";
export type {
  Client,
  ProcedureCallable,
  ProcedureWithKind,
  VoidIfInputNull,
} from "./createClient";
export { RpcError, isRpcError, safe } from "./error";
