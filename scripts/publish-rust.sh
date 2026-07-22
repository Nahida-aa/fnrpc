#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────────────────────
# publish-rust.sh — Bump workspace version and publish crates
#
# Reads the new version from packages/fnrpc-client/package.json
# (set by Changesets), updates workspace Cargo.toml, then publishes
# each crate to crates.io in dependency order.
#
# Usage:
#   ./scripts/publish-rust.sh
#
# Environment:
#   CARGO_REGISTRY_TOKEN  — required for cargo publish
# ──────────────────────────────────────────────────────────────

if [ -z "${CARGO_REGISTRY_TOKEN:-}" ]; then
  echo "Error: CARGO_REGISTRY_TOKEN is not set"
  exit 1
fi

# ── Resolve versions ──────────────────────────────────────────

NEW_VERSION=$(jq -r '.version' packages/fnrpc-client/package.json)
CURRENT_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml)

if [ "$NEW_VERSION" = "$CURRENT_VERSION" ]; then
  echo "Version unchanged ($CURRENT_VERSION) — nothing to publish"
  exit 0
fi

echo "Bumping Rust workspace from $CURRENT_VERSION → $NEW_VERSION"

# ── Bump workspace version ────────────────────────────────────

sed -i "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" Cargo.toml

# Update workspace dependency versions so published crates resolve correctly
sed -i \
  "s/fnrpc = { version = \"$CURRENT_VERSION\"/fnrpc = { version = \"$NEW_VERSION\"/" \
  Cargo.toml
sed -i \
  "s/fnrpc-macros = { version = \"$CURRENT_VERSION\"/fnrpc-macros = { version = \"$NEW_VERSION\"/" \
  Cargo.toml

git add Cargo.toml
git commit -m "chore: bump Rust crates to v$NEW_VERSION"

# Push the version bump back to main so the workspace version stays in
# sync with the published crates (otherwise the next release reads a stale
# CURRENT_VERSION and never advances).
git pull --ff-only
git push origin main

# ── Publish in dependency order ────────────────────────────────

echo "Publishing fnrpc-macros..."
cargo publish -p fnrpc-macros

echo "Publishing fnrpc..."
cargo publish -p fnrpc

echo "Publishing fnrpc-axum..."
cargo publish -p fnrpc-axum

echo "Publishing fnrpc-xitca..."
cargo publish -p fnrpc-xitca

echo "Publishing fnrpc-web..."
cargo publish -p fnrpc-web

echo "Publishing fnrpc-tauri..."
cargo publish -p fnrpc-tauri

echo "All crates published successfully!"
