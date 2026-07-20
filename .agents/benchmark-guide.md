# Benchmark Guide for fnrpc

## Heap Allocation Analysis (dhat)

```bash
# Available scenarios
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-macro 1000   # fnrpc-web single router
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-mw 1000      # fnrpc-web + middleware
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-multi 1000   # fnrpc-web multi router + static
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-noop-raw 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-post 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-web-manual 1000
cargo run -p benches --bin dhat_compare --features dhat-heap -- fnrpc-xitca-web 1000   # fnrpc on xitca-web
cargo run -p benches --bin dhat_compare --features dhat-heap -- xitca-web 1000          # plain xitca-web
cargo run -p benches --bin dhat_compare --features dhat-heap -- xitca-web-mw 1000       # xitca-web + middleware
cargo run -p benches --bin dhat_compare --features dhat-heap -- xitca-web-multi 1000    # xitca-web multi + static
```

## Current Results (per 1000 ops)

| Benchmark | fnrpc-web | fnrpc-xitca-web | fnrpc-axum | xitca-web | axum |
|---|---|---|---|---|---|
| echo_get | 845B, 6blks | 1,369B, 9blks | 2,496B, 22blks | 1,048B, 8blks | 2,373B, 20blks |
| echo_get + mw | 845B, 6blks | — | — | 1,048B, 8blks | — |
| echo_get (multi + static) | 1,042B, 8blks | — | — | 1,048B, 8blks | — |
| noop_raw | 96B, 2blks | 624B, 5blks | — | 176B, 2blks | — |

Key insight:
- fnrpc-web single router has zero Box::pin (845B) — fastest of all frameworks
- fnrpc-web multi router adds one Box::pin at route boundary (1,042B), matching xitca-web
- fnrpc-xitca-web has xitca-web framework tax (+321B vs fnrpc-web)
- fnrpc-axum ≈ axum native (+123B), framework tax already high
- **fnrpc-web is 2.8× more allocation-efficient than axum**
