# fnrpc Roadmap

## Auto-regenerate TypeScript bindings

**Problem:** Currently `bun run gen-fnrpc` must be run manually after every Rust RPC change to regenerate `bindings.ts`. This is easy to forget and breaks type safety.

**Goal:** Automatic re-generation on Rust file changes, similar to `@tanstack/router-plugin` for route tree types.

### Option A: Vite plugin (`@fnrpc/codegen`)

A new npm package providing a Vite plugin:

```ts
// vite.config.ts
import { fnrpcCodegen } from "@fnrpc/codegen/vite";

export default defineConfig({
  plugins: [
    fnrpcCodegen({ command: "cargo run --bin gen-fnrpc" }),
  ],
});
```

Implementation:
- `buildStart()`: run gen-fnrpc command on startup
- `configureServer()`: watch `src-tauri/**/*.rs` via chokidar or fs.watch
- HMR integration: trigger full reload on regeneration

Pros: best DX, similar to `@tanstack/router-plugin`
Cons: new npm package to maintain, Vite-specific

### Option B: `cargo watch` + concurrently

```json
{
  "scripts": {
    "dev": "concurrently \"vite\" \"cargo watch -x 'run --bin gen-fnrpc'\""
  }
}
```

Pros: zero new code, works today
Cons: no HMR integration, separate terminal process

### Recommendation

Start with Option A when there's demand. Option B is available as an immediate workaround.
