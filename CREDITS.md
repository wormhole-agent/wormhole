# Credits

WormHole stands on three layers of work that came before it:

1. **Direct dependencies**: the libraries we link against and the binaries we ship next to ours.
2. **Inspirations**: named people and organizations whose ideas, papers, codebases, or working styles shaped specific decisions in this project.
3. **Contributors**: the humans who land patches.

Every public release keeps this file in sync with `NOTICE`. CI will fail the release on drift once the auto-generator lands.

---

## Tier 1: Direct dependencies

### Rust crates (direct, from `wormhole/Cargo.toml`)

These 23 crates are the ones the binary explicitly depends on. The full transitive closure is regenerated at release time and lives in the build output as `credits-tier1-full.json`. `cargo about generate` is the recommended generator.

- async-trait v0.1 (MIT OR Apache-2.0): async functions in traits, used by the provider abstraction.
- anyhow v1 (MIT OR Apache-2.0): application-level error wrapping.
- axum v0.7 (MIT): HTTP server framework. Powers the embedded dashboard and `/api/*` surface.
- chrono v0.4 (MIT OR Apache-2.0): date/time handling for cron, logs, daily memory files.
- clap v4 (MIT OR Apache-2.0): command-line parsing for `serve`, `test`, `cron-list`, `ask`, etc.
- dirs v6 (MIT OR Apache-2.0): cross-platform user directory paths.
- futures v0.3 (MIT OR Apache-2.0): async combinators.
- once_cell v1 (MIT OR Apache-2.0): lazy statics for global config.
- regex v1 (MIT OR Apache-2.0): text matching for log filters and tool argument parsing.
- reqwest v0.12 (MIT OR Apache-2.0): HTTP client. Used by every provider, the Telegram bot, and the `http_get` tool.
- serde v1 (MIT OR Apache-2.0): serialization framework.
- serde_json v1 (MIT OR Apache-2.0): JSON I/O for cron state, sessions, dashboard data.
- shell-escape v0.1 (MIT OR Apache-2.0): argument quoting for the `shell` tool.
- thiserror v2 (MIT OR Apache-2.0): derive macro for error types.
- tokio v1 (MIT): async runtime. Drives every concurrent path in the binary.
- tokio-cron-scheduler v0.13 (MIT OR Apache-2.0): cron expression parsing and tokio-integrated scheduling.
- toml v0.8 (MIT OR Apache-2.0): config file parsing for `config.toml` and `cron.toml`.
- tower-http v0.6 (MIT): HTTP middleware (CORS layer, file-serving layer).
- tracing v0.1 (MIT): structured logging API.
- tracing-appender v0.2 (MIT): rolling log file writer.
- tracing-subscriber v0.3 (MIT): log filter and formatter.
- url v2 (MIT OR Apache-2.0): URL parsing for provider base URLs.
- uuid v1 (MIT OR Apache-2.0): session and request IDs.

### Workspace Node dependencies (from `workspace/tools/package.json`)

These npm packages back the small subset of workspace tools that ship in the public template. The list is short by design: only what the shipped tools actually require.

- (filled in at release time by `build/credits-tier1.js` once it lands)

### Bundled binary blobs

None in Phase 1. Build-from-source users supply their own Rust toolchain and (optionally) their own Node + npm + Ollama + yt-dlp.

---

## Tier 2: Inspirations

These are named people and organizations whose work directly shaped specific parts of this codebase. The bar is "their idea or code is visible in the project, and a reader who reads both can see the influence." This list is curated by PR with a reviewer-checked "yes, this person actually shaped that part of the code" gate.

### Andrej Karpathy

The autoresearch pattern. Karpathy's recurring point that you can teach a small model new behaviors by giving it an iterative loop, an evaluator, and a memory of past attempts shaped how WormHole runs its scheduled research jobs. The harness is a small loop, the model's job is to make one move per turn, and the workspace is the memory. We did not lift code; we lifted the operating shape. See `dreaming.rs` and the cron entries under `wormhole/cron.d/`.

### Mo Gawdat

Squash mode. Gawdat's framing of "do the work in concentrated bursts, then rest, do not pretend you can sustain peak output for eight hours" became the operating tempo for the agent itself. WormHole's cron jobs are deliberately bursty: hard work for a few minutes, then quiet. The dreaming pass at 03:00 is squash mode at the day scale: one big consolidation pass, then nothing until tomorrow. See `workspace/AGENTS.md` and the cron schedules.

### Anthropic

Claude, the API surface, the skills system, prompt caching, the markdown-skill pattern. WormHole's primary provider is Anthropic Claude, and the binary's tool-calling iteration loop is shaped around Anthropic's tool-use protocol (`brain.rs`). The skills system in `skills/` is a direct descendant of Anthropic's published skill format. The MCP-style tool-call envelope is the reference for the internal tool surface. Prompt caching support in `[prompt_caching]` (config.toml) mirrors Anthropic's `ephemeral` and `1h` cache tiers.

### OpenAI

The chat-completions API shape. The OpenAI-compatible provider in `providers/openai_compat.rs` handles both OpenAI itself and DeepSeek (same wire format, different `base_url`). Function calling, the message role taxonomy, and the streaming chunk format are all OpenAI's contributions to the field.

### DeepSeek

The V4-Pro-Thinking critique gate. WormHole runs a separate critique pass through DeepSeek's V4-Pro for any high-stakes write (memory promotion, public-doc drafting). The pattern of "have one model produce, have a different model critique" came out of testing DeepSeek's strengths as a heavy reviewer. See `workspace/skills/chained-research/` and `workspace/tools/deepseek.js`.

### Ollama

Local LLM runtime detection and the `/api/tags` endpoint shape. WormHole's `providers/ollama.rs` talks to a local Ollama instance using Ollama's own API. The first-run flow probes `127.0.0.1:11434/api/tags` and offers to wire detected models as the local fallback. Ollama is the "you can run an agent fully offline" backstop in this project.

### Brave

The free-tier web search API used by `workspace/tools/web-search.js`. Brave Search lets the agent ground itself in current information without funneling every query through Google or Bing. The free-tier quota is generous enough for personal use, which matters for a local-first agent that some users will run without ever paying anyone.

### OpenClaw (direct lineage)

The previous-generation system that WormHole evolved out of. The install pattern, the workspace layout (`AGENTS.md` / `MEMORY.md` / `SOUL.md` / daily memory files), the dreaming/promotion memory model, the tier-routing notion, and the "agent edits its own skills folder" loop all started in OpenClaw. WormHole is what we would build now if we were starting fresh, but it stands on OpenClaw's shoulders. The `/skills` markdown convention is OpenClaw's idea.

### Hermes Agent (NousResearch)

Autonomous skill curation. NousResearch's Hermes Agent v0.12 "Curator" release crystallized the pattern of skills-as-curated-artifacts that an agent maintains over time, rather than as static prompts handed down by humans. WormHole's `workspace/skills/self-improve/` skill is the in-house version of that loop. The prompt-caching block in `config.toml` is a port of Hermes's `prompt_caching` config shape. The tier-routing intuition (cheap model for triage, deep model for reasoning) also draws on Hermes's published model-tiering work.

---

## Tier 3: Contributors

Generated from `git log` at release time. For v0.1.0 this list is seeded by hand and replaced by the auto-generated version on first release.

```
Phase 1 placeholder. Will be auto-populated from git log after the public repo opens.
- Initial author and project lead.
```

---

## Notes on the auto-generation pipeline

The Tier 1 list above is hand-curated for v0.1.0. The release pipeline (Phase 1 follow-up) will generate it from:

- `cargo about generate --config build/about.toml --format markdown > build/credits-tier1-rust.md`
- `npx license-checker --production --json > build/credits-tier1-node.json`, then a small formatter.

CI fails the build if the generated output drifts from the committed `CREDITS.md`. CI also fails if any `LICENSE-UNKNOWN` entries remain in a release build.

---

End of credits.
