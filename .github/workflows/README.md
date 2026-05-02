# Workflows

Three workflows live here. All three run as of v0.1.1 (source-import PR).

## `ci.yml`
Builds and tests the Rust binary on every push and pull request to `main`:

- `cargo build --release` (default features, includes `dashboard`)
- `cargo build --release --no-default-features` (sanity check the minimal profile)
- `cargo test`
- `cargo clippy -- -D warnings`

A second job syntax-checks each `workspace/tools/*.js` file with `node --check`.

## `release.yml`
On `v*` tag pushes, builds the unsigned `wormhole.exe`, computes a SHA256
checksum, packages a zip with `README.md` + `LICENSE` + `NOTICE` + `PRIVACY.md`,
and attaches both to a GitHub Release. The body is read from `CHANGELOG.md`.

Commented placeholders mark where Authenticode signing and Ed25519 manifest
signing slot in once their respective keys are provisioned in GitHub Secrets.
See BUILD-GUIDE-v2.3 Section 9 for the rationale.

## `secret-scan.yml`
Trufflehog full-filesystem scan on every push and PR. Uses a full scan instead
of a diff scan so the very first commit on a fresh repo gets covered too —
see comments inside the file for why.
