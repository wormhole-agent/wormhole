# WormHole

WormHole is a binary that runs your AI assistant locally. It remembers things, runs jobs on a schedule, calls big models when it needs to, and serves a small dashboard so you can see what it is doing. It is one Rust process: it talks to LLM providers, writes to a Telegram bot, runs cron jobs, and serves the dashboard at `127.0.0.1:18790`. There is no cloud, no telemetry, no sign-up, no third-party server. The agent runs on your machine and the keys live on your machine.

The point is to give one person a thing that behaves like an assistant: it remembers what you told it last week, it does small recurring jobs without being asked, and when you give it something hard it can hand off to a bigger model. It is built to be edited. Skills are markdown files. Tools are short Node scripts. Cron jobs are TOML entries. The dashboard is one HTML file. If you want to change how it behaves, you open a text editor.

## Core principles

- **No telemetry, ever.** WormHole sends no data off your machine unless you explicitly invoke a tool that does so (an LLM provider call you configured, a search you ran). The product collects nothing about its own usage. This is not a default we might flip later; it is a project commitment. See [`PRIVACY.md`](./PRIVACY.md).
- **Local-first.** Memory, sessions, vault, and dashboard data all live on your disk. The vault is encrypted with Windows DPAPI bound to your account. The dashboard binds to loopback only.
- **Self-modifying.** The agent edits its own skills folder, proposes new memories nightly, and curates its own toolkit over time. A human approves promotion to long-term memory. The default rule is **the agent should look for, find, and build her own tools before asking you for help.**
- **Model-tiered.** Cheap models for triage, deep models for reasoning, local models (via Ollama) for offline fallback. The brain walks the configured fallback chain when a provider fails.

## Status

v0.1.0 is the first public release. Source code, docs, and unsigned binaries via GitHub Actions on each tag. The signed MSI installer with a first-run wizard is a Phase 2 deliverable, triggered when a non-developer wants to install or when the project hits enough signal to justify a code-signing certificate.

## Install paths

Two paths in v0.1.0. Pick whichever you are more comfortable with.

### Build from source (recommended for v0.1.0)

```
git clone https://github.com/wormhole-agent/wormhole
cd wormhole
cargo build --release
./target/release/wormhole serve
```

You need a Rust toolchain (stable, edition 2021 or newer). Optional: Node.js 20 LTS if you plan to use the workspace tools, and Ollama if you want a local LLM fallback. Full walkthrough in [`GETTING-STARTED.md`](./GETTING-STARTED.md).

### Download the unsigned binary

GitHub Releases publishes a zipped Windows binary for every tag. Download, unzip, run `wormhole.exe`. Windows SmartScreen will warn that the publisher is unrecognized; click "More info" then "Run anyway" to bypass. This is expected for v0.1.0 because the binary is unsigned. The Phase 2 release will be Authenticode-signed and SmartScreen will trust it after a reputation warmup.

### Phase 2 (not yet available)

A signed `.msi` installer with a first-run wizard, scheduled-task registration, and a one-click upgrade path. Trigger: first real non-developer who wants to install, or 50+ stars on the repo, whichever comes first.

## How WormHole works

There are three pieces, and they only make sense together.

1. **The binary** (`wormhole.exe`). One Rust process. Talks to LLM providers, runs cron, exposes `/api/*` on `127.0.0.1:18790`, serves the dashboard from the same port.
2. **The dashboard** (BrainWorms). One HTML page plus two config files (`widgets.json`, `nodes.json`). Reads JSON snapshots written by cron jobs and renders a grid of widgets.
3. **The workspace.** A directory tree the agent reads from and writes to during normal operation. Holds the personality (`SOUL.md`), the operating rules (`AGENTS.md`), long-term memory (`MEMORY.md`), daily session logs, Node-based tools, markdown skills, and module folders for project-specific work.

The default `workspace/` ships with one **example-module** so you can see the shape. You add your own modules over time by copying the example.

## Default features and the minimal build

The dashboard is on by default. `cargo build --release` builds the full thing including the dashboard. If you want a Telegram-only or CLI-only build (smaller binary, no dashboard routes, no embedded HTML), build with `--no-default-features`:

```
cargo build --release --no-default-features
```

## Where to go next

- **[`GETTING-STARTED.md`](./GETTING-STARTED.md)**: zero-to-first-conversation walkthrough. Read this if you have never seen the project before.
- **[Wiki](https://github.com/wormhole-agent/wormhole/wiki)**: architecture deep-dives, schema references, operations runbooks. The README tells you what; the wiki tells you why and how.
- **[`CONTRIBUTING.md`](./CONTRIBUTING.md)**: how to file a bug, propose a change, write a skill, ship a patch. The readability standard at the top is a hard rule.
- **[`SECURITY.md`](./SECURITY.md)**: disclosure channel for security issues. Do NOT file public issues for security; use the private channel documented there.
- **[`PRIVACY.md`](./PRIVACY.md)**: the no-telemetry promise, written down so you can hold us to it.
- **[`SUPPORT.md`](./SUPPORT.md)**: best-effort support boundaries. There is no SLA.
- **[`CREDITS.md`](./CREDITS.md)**: every dependency, every named inspiration, every contributor. Three tiers, kept in sync with `NOTICE`.
- **[`CHANGELOG.md`](./CHANGELOG.md)**: per-version notes.
- **[`MAINTAINERS.md`](./MAINTAINERS.md)**: who is on point for what.

## License

Apache-2.0. See [`LICENSE`](./LICENSE) and [`NOTICE`](./NOTICE). The license was picked for the patent grant and the explicit attribution requirements; both matter for a public agent framework that other projects may embed.

## A note on names

The crate and binary are named `wormhole`. The default agent personality is "Larry" (you'll see this in `workspace/SOUL.md`). The dashboard is BrainWorms. **You can rename your worm to anything you like; edit `workspace/SOUL.md` to make the agent your own.** The personality file is meant to be customized.
