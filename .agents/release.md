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
- specta is pinned to `=2.0.0-rc.25` (latest on crates.io). `rc.26` exists
  only as a local checkout under `learn_ls/specta` and is **not** published,
  so do not bump specta until it is released.
