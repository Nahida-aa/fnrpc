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
3. If anything was published, the same workflow runs `scripts/publish-rust.sh`,
   which:
   - reads the new version from `packages/fnrpc-client/package.json`,
   - bumps the Rust workspace version in the root `Cargo.toml`
     (`[workspace.package].version` and the `fnrpc` / `fnrpc-macros`
     entries under `[workspace.dependencies]`),
   - `cargo publish`es every crate to crates.io **in dependency order**:
     `fnrpc-macros` → `fnrpc` → `fnrpc-axum` → `fnrpc-xitca` →
     `fnrpc-web` → `fnrpc-tauri`.

## Version sync: TS is the source of truth

Rust crate versions are **derived from** the `@fnrpc/client` TS package
version. `publish-rust.sh` does the sync — never set them by hand. If you bump
`Cargo.toml` manually, the script's `NEW_VERSION != CURRENT_VERSION` check will
either no-op or clobber your change.

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
- The cargo config replaces `crates-io` with the `ustc` mirror, so
  `cargo search` / `cargo update --precise` resolve against the mirror.
  `cargo publish` still targets crates.io (pass `--registry crates-io` if
  needed).
- specta is pinned to `=2.0.0-rc.25` (latest on crates.io). `rc.26` exists
  only as a local checkout under `learn_ls/specta` and is **not** published,
  so do not bump specta until it is released.
