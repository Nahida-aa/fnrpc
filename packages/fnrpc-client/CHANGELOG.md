# @fnrpc/client

## 0.2.0

### Minor Changes

- [`2105499`](https://github.com/Nahida-aa/fnrpc/commit/2105499c48355727fd71c185cbab557cc0b1c16a) Thanks [@Nahida-aa](https://github.com/Nahida-aa)! - Subscribe POST method — `#[rpc_subscribe("post")]` sends input in request body; transport detects method from `__procedureMeta`

  Unified procedure metadata — `__procedureMeta` replaces `__procedureKinds`, includes `{ kind, method }` for each procedure

  fetch-based SSE transport — replaces EventSource, supports POST body and AbortSignal

  AbortSignal improvements — SSE reader cancels on signal abort, proper cleanup on iterator return

  Snake-case consistency — client method names match Rust function names

  New example: axum-react-query — full-stack example with React + TanStack Query
