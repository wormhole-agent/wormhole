# Workflows

Three workflows live here. Only one runs in v0.1.0.

## Active

### `secret-scan.yml`
Trufflehog full-filesystem scan on every push and PR. Runs immediately on
the first commit because we use a full scan instead of a diff scan
(see comments inside the file for why).

## Disabled until source import (PR #1)

### `ci.yml.disabled`
Builds and tests the Rust binary plus syntax-checks the Node tools.
**Disabled** because the `wormhole/src/` tree in this skeleton is empty —
there is no `Cargo.toml` and no source. The first PR after this skeleton
goes live imports the actual source from the maintainer's local tree.
At that point, rename this file to `ci.yml` to re-enable.

### `release.yml.disabled`
Builds the unsigned `wormhole.exe` and attaches it to a GitHub Release
when a `v*` tag is pushed. **Disabled** for the same reason as `ci.yml`.
Rename to `release.yml` after source import. Tag `v0.1.0` once green.

## Why disable instead of delete?

Keeps the templates visible while reading the repo, makes the source-import
PR a one-line rename per workflow, and avoids "workflow failed" emails on
the very first push to a fresh repo.
