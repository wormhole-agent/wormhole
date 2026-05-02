# Updating

`wormhole self-update` checks the signed release manifest, downloads a new build, verifies its hash and signature, snapshots your workspace, applies the update atomically, runs a post-update doctor, and rolls back if anything fails. This page covers the manifest format, the Ed25519 release-key pinning, the four-phase update flow (preflight, stage, apply, commit-or-rollback), the migration runner that handles workspace schema changes, and the rollback command for when things go sideways.

<!-- TODO: fill in during Phase 1 docs sprint -->
