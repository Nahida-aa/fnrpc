---
"@fnrpc/client": minor
"@fnrpc/tanstack-query": minor
---

Subscribe POST method — `#[rpc_subscribe("post")]` sends input in request body; transport detects method from `__procedureMeta`

Unified procedure metadata — `__procedureMeta` replaces `__procedureKinds`, includes `{ kind, method }` for each procedure

fetch-based SSE transport — replaces EventSource, supports POST body and AbortSignal

AbortSignal improvements — SSE reader cancels on signal abort, proper cleanup on iterator return

Snake-case consistency — client method names match Rust function names

New example: axum-react-query — full-stack example with React + TanStack Query
