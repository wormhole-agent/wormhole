# wormhole/src/

The Rust source for the WormHole binary lives here.

## v0.1.0 status: source migration is the first PR after publish

The Phase 1 skeleton ships this directory as a placeholder. The actual `*.rs` files (about 6,700 lines across 13 modules) live in the maintainer's working tree and land here as the first PR after the public repo is created.

The first source-import PR does these mechanical changes and then drops the source in:

1. Rename the crate: `Cargo.toml` package and `[[bin]]` name to `wormhole`. The `Cargo.toml.example` in the parent directory shows the target shape.
2. Move dashboard-only routes behind `#[cfg(feature = "dashboard")]` per the embedded-server lock.
3. Sweep for the maintainer's pre-rename binary name and rename references that are about the binary (not the agent personality).
4. Drop the source files into this directory.
5. Run `cargo build --release` and `cargo test`. CI catches anything that breaks.

## Module map (for orientation; full audit in the build-guide phase-0 archive)

| File | Role | Approx lines |
|---|---|---:|
| `main.rs` | CLI entry. `serve`, `init`, `ask`, `cron-list`, `cron-run`, `vault edit/export/import-export`, `doctor`. | 425 |
| `config.rs` | TOML config loader. | 509 |
| `brain.rs` | The respond loop. Provider calls, tool-use iterations, history slicing. | 832 |
| `cron.rs` | Scheduled jobs runner. | 556 |
| `tools.rs` | Tool surface and path sandboxing. | 909 |
| `ui.rs` | The embedded HTTP server. axum routes for `/api/*` and the dashboard. | 1825 |
| `telegram.rs` | Telegram bot. Long-poll, command parsing. | 464 |
| `providers/mod.rs` + `anthropic.rs` + `ollama.rs` + `openai_compat.rs` | LLM provider trait + three implementations. | ~600 |
| `dreaming.rs` | Nightly memory consolidation. | 630 |
| `subagent.rs` | Sub-agent helper used by skills that delegate. | 242 |
| `skills.rs` | Markdown-skill loader. | 212 |
| `memory.rs` | Memory file readers. | 81 |
| `error.rs` | Error/Result type aliases. | 27 |

## What you should NOT find here once the source lands

- No personal-name references in source, comments, or log strings.
- No business-module names from any private fork.
- No hardcoded user-home paths.
- No provider keys, tokens, or vault contents.

CI grep enforces the first three on every push.
