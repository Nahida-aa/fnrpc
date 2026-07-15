# Changesets

This directory manages versioning for the `@fnrpc/*` packages via [Changesets](https://github.com/changesets/changesets).

## Workflow

1. Run `bun run changeset` to create a new changeset
2. Commit the generated markdown file
3. On release, run `bun run version-packages` to bump versions and update changelogs
4. Commit the version bump, then publish with `bun run --filter '@fnrpc/*' publish`
