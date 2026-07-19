# Roadmap

## Performance

- [ ] **`layer_fn` 消除 `Box::pin` 样板** — 当 Rust 稳定 `AsyncFn` trait（`async fn` in traits）后，`layer_fn` 的闭包可以写成 `async |inner, ctx, path, input, is_get, extensions| { ... }`，不需要 `Box::pin(async move { ... })`。跟踪 issue: <https://github.com/rust-lang/rust/issues/29625>
