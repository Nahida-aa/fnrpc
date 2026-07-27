#!/usr/bin/env bash
# Publish the fnrpc TS packages to npm.
#
# Usage:
#   ./scripts/publish-ts.sh                  # builds then publishes every workspace package
#   ./scripts/publish-ts.sh @fnrpc/client    # builds then publishes only the named package
#
# This script ALWAYS runs `bun run build` before publishing. The published
# artifact is the compiled `dist/` output — skipping the build is what
# previously shipped stale/broken code (the envelope-unwrapping fix never
# reached npm because `dist/` was never rebuilt).
#
# Publishing uses `bun publish` (NOT `npm publish` / `changeset publish`):
# bun understands the bun-only `workspace:*` protocol and rewrites it to the
# real package version in the tarball, whereas npm/changeset ship `workspace:*`
# verbatim (broken install for consumers).
#
# Authentication:
#   - A project-level `.npmrc` (gitignored) must contain
#       //registry.npmjs.org/:_authToken=<token>
#     where <token> is an npm token with "publish" rights and 2FA-to-publish
#     DISABLED (granular token → turn OFF "Require 2FA to publish"). bun reads
#     `.npmrc` and publishes without prompting for an OTP.
#   - For CI, the same token is provided via the NPM_TOKEN secret written into
#     `.npmrc` before this script runs (see release.yml).
#
# NOTE: `bun install` does NOT sync workspace-internal package versions into
# `bun.lock`, so we rebuild the lock first to guarantee `workspace:*` resolves
# to the version just bumped.

set -euo pipefail

cd "$(dirname "$0")/.."

PKG="${1:-}"

echo "==> Syncing workspace versions into bun.lock (so workspace:* resolves correctly)"
rm -f bun.lock
bun install

echo "==> Building all TS packages (required before publish)"
bun run build

# Resolve the list of packages to publish.
if [ -n "$PKG" ]; then
  PKGS=("$PKG")
else
  mapfile -t PKGS < <(find packages -maxdepth 2 -name package.json -not -path '*/node_modules/*' | sed 's#/package.json##')
fi

for p in "${PKGS[@]}"; do
  name=$(node -p "require('./$p/package.json').name")
  version=$(node -p "require('./$p/package.json').version")
  echo "==> Publishing $name@$version"
  # `--tolerate-republish` keeps re-runs safe when the version already exists.
  (cd "$p" && bun publish --access public --tolerate-republish)
done

echo "==> Done."
