---
"@fnrpc/client": minor
---

Upgrade specta to `2.0.0-rc.26` (tracked via `[patch.crates-io]` in the workspace `Cargo.toml`, the same git rev tauri-specta uses, since rc.26 is not yet published to crates.io).

Codegen now applies serde attributes to the generated TypeScript through `specta-serde::PhasesFormat`, so `#[serde(rename = ...)]` — including **enum variant renames** — now appear in the generated `bindings.ts` (previously a no-op `Format` silently dropped them). BigInt-style Rust integers (u64/i64/u128/i128/usize/isize) are remapped to TS `bigint` via `specta-util::Remapper`, preserving lossless round-trips.
