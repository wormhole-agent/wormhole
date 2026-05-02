# Privacy

**No telemetry, ever.** This is not a default we might flip later. It is a project commitment.

## What this means in plain language

WormHole sends no data off your machine unless **you** explicitly invoke a tool that does so. Examples of explicit invocations:

- An LLM provider call you configured (Anthropic, OpenAI, DeepSeek, etc.). Your prompt and your data go to that provider, governed by your contract with them.
- A search call you ran (`web-search` tool, Brave / DuckDuckGo / SearXNG). The query and result-fetch go to that search provider.
- A Telegram message you sent (or that a cron job you wrote sent). The message body goes to Telegram.
- An update check you ran (`wormhole self-update`, when wired). The check fetches a release manifest from the project's update mirror.

WormHole **itself** collects nothing about its own usage. There is:

- No first-run install ping.
- No anonymous usage counter.
- No crash reporter that phones home.
- No "is this version current?" check that runs without you asking.
- No analytics SDK in any binary, dashboard, or workspace tool.
- No telemetry in `wormhole doctor`, which by default makes zero outbound network calls.

The doctor command has an `--online` flag for users who want it to ping providers, the update manifest, etc. That flag is opt-in and prompts before each call.

## Where this is enforced

Three places. Read them together if you want to verify the promise.

1. **The source.** The release CI greps the dependency tree (Rust + JS) and the source for known telemetry SDK names and known analytics endpoints. The release fails if any unexpected outbound endpoint or telemetry SDK is detected. The allowed outbound destinations are explicitly enumerated in `build/outbound-allowlist.toml` (LLM provider APIs, Telegram, the user's own update-manifest URL).
2. **The doctor command.** `wormhole doctor` is local-only by default. The implementation does not perform DNS, provider API calls, Telegram calls, update checks, or web requests unless `--online` is passed.
3. **The first-run flow.** The Phase 2 wizard never makes any outbound network call other than user-driven probes (the Telegram getMe probe, the Ollama probe, the user-explicit provider key test). No install-success ping. No telemetry sent on completion.

## What you should still expect

- Your LLM providers see your prompts. Anthropic, OpenAI, DeepSeek, etc., have their own privacy policies. Read those if you care.
- Your Telegram bot's messages traverse Telegram's infrastructure. Read Telegram's policies.
- If you write a workspace tool that calls an external service, that tool sends your data to that service. The agent runs YOUR tools.
- If you turn on `wormhole self-update` and the project later moves the update manifest to a different host, you'll talk to that host on each update check.

In other words: the project does not collect anything; you may, through your configured providers and tools, hand data to third parties on purpose.

## What to do if you find something that violates this

This is an Apache-2.0 OSS project. If you find a code path that sends data off-machine without an explicit user action, file a private GitHub Security Advisory at `https://github.com/wormhole-agent/wormhole/security/advisories/new`. The release CI's telemetry grep is the safety net; if it ever misses something, that is a release-blocking bug.

## What might change in the far future

If, at some point, an opt-in privacy-respecting telemetry option is ever proposed:

- It must live behind an off-by-default opt-in flag.
- It must show a separate privacy notice as part of the opt-in flow.
- It must go through an RFC plus community discussion before it is built.
- It must clear the "is this still consistent with the v0.1.0 hard promise?" bar.

For the foreseeable future: no telemetry, ever.

## Differentiator framing

Most commercial agent frameworks ship telemetry on by default and ask forgiveness later. WormHole does not. This is a deliberate position. The agent that doesn't watch you is a feature, not an absence.
