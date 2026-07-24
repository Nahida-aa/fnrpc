# Release Process for fnrpc

Publishing is fully automated via GitHub Actions. **Do not run `cargo publish`
by hand**, and **do not manually bump the Rust crate versions** — both are
driven by the Changesets workflow and will conflict with CI.

## How a release happens

1. A changeset file is added under `.changeset/*.md` describing the change
   (e.g. `.changeset/fix-foo.md` with `"@fnrpc/client": patch`).
2. On merge to `main`, `.github/workflows/release.yml` runs `changesets/action`:
   - `changeset version` bumps the **TypeScript** package versions
     (`packages/fnrpc-client/package.json`, etc.).
   - `changeset publish` publishes the TS packages to npm.
3. The same workflow then runs `scripts/publish-rust.sh` (it runs
   unconditionally now — see Pitfalls). It:
   - reads the new version from `packages/fnrpc-client/package.json`,
   - if that differs from the Rust workspace version in the root `Cargo.toml`,
     bumps it (`[workspace.package].version` and the `fnrpc` / `fnrpc-macros`
     entries under `[workspace.dependencies]`), then `git push`es the bump
     back to `main`,
   - `cargo publish`es every crate to crates.io **in dependency order**:
     `fnrpc-macros` → `fnrpc` → `fnrpc-axum` → `fnrpc-xitca` →
     `fnrpc-web` → `fnrpc-tauri`.

   No-ops (exits 0) when the Rust version already matches the TS version, so
   re-running the workflow without a version change is safe.

## Version sync: TS is the source of truth

Rust crate versions are **derived from** the `@fnrpc/client` TS package
version. `publish-rust.sh` does the sync — never set them by hand. If you bump
`Cargo.toml` manually, the script's `NEW_VERSION != CURRENT_VERSION` check will
either no-op or clobber your change.

The workspace `Cargo.toml` version is the **one and only** place the Rust
version lives (crates use `version.workspace = true`). After a release it must
read the same number as `packages/fnrpc-client/package.json`. `publish-rust.sh`
commits **and pushes** the bump back to `main`, so it should stay in sync
automatically — but verify it after a release (see Pitfalls for the historical
bug where it did not).

## What an agent should do to ship a fix

- Make the code fix (e.g. in `crates/fnrpc/src/...`).
- Add a changeset: `bun run changeset` (or hand-write a `.changeset/*.md`).
- Commit the fix **and** the changeset together. Do **not** touch `Cargo.toml`
  versions or `CHANGELOG` (Changesets generates those).
- Open a PR / push to `main`; CI versions and publishes both ecosystems.

## Pitfalls

- `cargo publish` locally will authenticate against crates.io and push an
  irreversible publish — and it will be out of sync with the TS version.
  Let CI do it.
- `publish-rust.sh` must run **unconditionally**, not gated on
  `changesets.outputs.published`. Under `changesets/action`'s direct-push
  mode that output is unreliable (it was often `false` even after a
  successful npm publish), which silently skipped the Rust publish and left
  the Rust crates one or more versions behind npm. The step runs every time
  now; the version-match check inside the script is what makes re-runs safe.
- `publish-rust.sh` must `git push` the `Cargo.toml` bump back to `main`.
  A previous version only `git commit`ted it, so the bump never landed and
  the next release still saw the stale `CURRENT_VERSION` — the workspace
  `Cargo.toml` got stuck at `0.3.0` while crates.io/npm were already at
  `0.3.3`. If after a release `Cargo.toml` still shows the old version,
  the push is missing, not the bump.
- Aligning by hand: if the workspace `Cargo.toml` has fallen behind (e.g.
  stuck at `0.3.0` while the TS packages are at `0.3.3`), first fast-forward
  `main` to the `changeset-release/main` `chore: version packages` commit (so
  the TS packages reflect the already-published version), then let a normal
  push run the workflow — do **not** hand-edit `Cargo.toml` to the new number,
  or the script will see equal versions and skip publishing the missing Rust
  release.
- The cargo config replaces `crates-io` with the `ustc` mirror, so
  `cargo search` / `cargo update --precise` resolve against the mirror.
  `cargo publish` still targets crates.io (pass `--registry crates-io` if
  needed).
- specta is pinned to `=2.0.0-rc.26` (the same git rev tauri-specta uses) via a
  `[patch.crates-io]` block in the workspace `Cargo.toml`. **rc.26 is NOT on
  crates.io yet** — it is only available from the specta git repo. tauri-specta
  uses the identical setup and is likewise blocked from publishing an rc.26
  build to crates.io (its crates.io releases stop at rc.21).

## TEMPORARY: manual publish until specta rc.26 hits crates.io

The automated `release.yml` run currently **fails** at `publish-rust.sh` with:

```
error: failed to select a version for the requirement `specta = "=2.0.0-rc.26"`
candidate versions found which didn't match: 2.0.0-rc.25, 2.0.0-rc.24, ...
required by package `fnrpc v0.4.0`
```

`cargo publish` ignores `[patch.crates-io]`, so a crate depending on
`specta = "=2.0.0-rc.26"` cannot be published to crates.io until specta-rs
ships rc.26 there. **Do not rely on the automated workflow for now.**

Until then, publish by hand from a clean checkout of `main` (this is the one
case where manual `cargo publish` is allowed, overriding the "never by hand"
rule above):

```bash
# From repo root, after `cargo login` (or CARGO_REGISTRY_TOKEN set):
cargo publish -p fnrpc-macros
cargo publish -p fnrpc
cargo publish -p fnrpc-axum
cargo publish -p fnrpc-xitca
cargo publish -p fnrpc-web
cargo publish -p fnrpc-tauri
```

Publish **in dependency order** (macros first; `fnrpc` before its integrations).
`cargo publish` waits for each crate to become available on the index before
the next dependent one can resolve, so run them sequentially.

Notes / state as of this writing:
- The Rust workspace and TS packages are already at **0.4.0** (bumped by the
  partial `release.yml` run). `fnrpc-macros 0.4.0` was published to crates.io;
  the rest of the 0.4.0 crates are NOT (the run failed on the `specta` req).
- That leaves `fnrpc-macros 0.4.0` published while `fnrpc` 0.4.0 is absent — a
  broken partial publish. Once specta rc.26 is on crates.io, re-run the manual
  publish above (or re-enable `release.yml`) to fill in the remaining 0.4.0
  crates; the version numbers already line up, so no downgrade / re-version is
  needed. Do **not** yank `fnrpc-macros 0.4.0` — it just holds the 0.4.0 slot.
- The TS packages (npm `@fnrpc/client`) were NOT published for 0.4.0 either
  (the changesets publish step was skipped when the job failed). Publish them
  manually too if needed: `bun install && bun run build && bun x changeset
  publish` (or `bun publish` from `packages/fnrpc-client`).

When specta rc.26 lands on crates.io: remove the `[patch.crates-io]` block from
the workspace `Cargo.toml`, switch the `specta`/`specta-typescript`/
`specta-serde`/`specta-util` deps to plain crates.io versions (`=2.0.0-rc.26` /
`0.0.13`), and re-enable the automated `release.yml` flow.
