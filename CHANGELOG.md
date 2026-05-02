# Changelog

All notable changes to WormHole will be listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

Nothing yet beyond the v0.1.0 release entry.

## [0.1.0] - unreleased

The first public release. Source-only repo; build-from-source as the primary install path; unsigned binaries on GitHub Releases as the secondary path.

### Added

- Public Apache-2.0 OSS release. Three-tier credits, NOTICE file, contributor on-ramp.
- Rust binary `wormhole` (renamed from internal `larry`). One process: brain, cron, Telegram, embedded dashboard, dreaming pass.
- Embedded BrainWorms dashboard. Default-on cargo feature (`dashboard`); `cargo build --release --no-default-features` for a minimal build.
- Workspace template with one `example-module/`, six generic skills, ten generic tools.
- The "find or build your own tools before asking the user" operating principle, encoded in `workspace/skills/find_or_build_capability.md` and reinforced in README, AGENTS.md, and GETTING-STARTED.
- DPAPI-encrypted secrets vault (Windows-only at v0.1.0). Cross-platform `age` vault planned for a later release.
- Loopback-only API + dashboard binding. `.token` auth on every `/api/*` route. CORS hard-allowlist.
- Local-only `wormhole doctor` by default. `--online` flag is opt-in.
- GitHub Actions CI: build, test, clippy, gitleaks secret scan. Unsigned binaries on tag.
- Wiki seed: 29 stub pages across Architecture, Concepts, Operations, Reference, Modules, Credits, Changelog, Getting Started.

### Privacy and security

- **No telemetry.** No first-run ping, no usage counter, no crash reporter, no analytics SDK. Documented in `PRIVACY.md` as a hard project commitment.
- Release manifest signing via Ed25519 (separate from Authenticode). Update verification works on unsigned binaries.
- Per-tool capability allowlist for cron jobs.

### Known limitations at v0.1.0

- Windows-only. macOS and Linux land in a later release via Tauri.
- Unsigned binaries; SmartScreen will warn on first run. Click "More info" then "Run anyway."
- No MSI installer. Build-from-source is the recommended path.
- No first-run wizard. `wormhole init` plus `wormhole vault edit` is the bootstrap.
- Workspace tools are generic Node scripts; the curated set is small by design. The agent is expected to look for, find, and build her own tools before asking you for help.

### Migration notes

- For users coming from the internal `larry` build: the binary is now `wormhole.exe`. The `~/wormhole/` home directory and `.token` path are unchanged. The crate-level rename does not affect the installed paths or the workspace layout.

---

[Unreleased]: https://github.com/wormhole-agent/wormhole/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/wormhole-agent/wormhole/releases/tag/v0.1.0
