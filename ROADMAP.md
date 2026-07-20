# Roadmap

## Performance

- [x] **中间件零开销重构** — 去掉 `Arc<dyn ErasedRpcService>`，`RpcRouter` 泛型化，中间件链 monomorphized
- [x] **`HookLayer` 消除 `input.to_vec()`** — before hook 签名改为 `(&[u8]) -> Result<&[u8], RpcErr>`，不修改 input 时零分配
- [x] **`fnrpc-web` 多路由支持** — `App::build().rpc().static_dir()`，radix tree 匹配，一次 `Box::pin`
- [ ] **`layer_fn` 消除 `Box::pin` 样板** — 当 Rust 稳定 `AsyncFn` trait（`async fn` in traits）后，`layer_fn` 的闭包可以写成 `async |inner, ctx, path, input, is_get, extensions| { ... }`，不需要 `Box::pin(async move { ... })`。跟踪 issue: <https://github.com/rust-lang/rust/issues/29625>

## Development

- [x] **`fnrpc-xitca`** — `FnrpcState` + `handle` 函数，挂载 fnrpc 到 xitca-web
- [x] **`fnrpc-axum`** — `FnrpcState` + `handle` 函数，挂载 fnrpc 到 Axum（2,496B/22blks vs axum 原生 2,373B/20blks）
- [ ] **`fnrpc-web` 静态文件优化** — 用 `http-file` crate 替换手写的 `tokio::fs::read`
- [ ] **文档** — `SUMMARY.md` 更新，API 文档完善
