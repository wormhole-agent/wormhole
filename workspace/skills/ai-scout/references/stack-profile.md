# Current Stack Profile (template)

> The ai-scout skill reads this to know what hardware and software you have, so it can filter findings for relevance. Update this file when your stack changes.

## Hardware

- **GPU**: (e.g. NVIDIA RTX 4060 8 GB VRAM, or "no GPU")
- **RAM**: (e.g. 32 GB)
- **CPU**: (e.g. desktop-class x86_64)
- **Storage free**: (e.g. ~600 GB)
- **OS**: (e.g. Windows 11, Ubuntu 24.04, macOS 14)

## Software stack

- **Agent runtime**: WormHole v0.1.0
- **Main brain**: (your top-tier provider, e.g. Claude Sonnet 4.6 via Anthropic API)
- **Fallback brain**: (e.g. Claude Opus, GPT-5.5)
- **Local model**: (e.g. Llama 3.2 3B via Ollama, or "none")
- **Embeddings**: (e.g. nomic-embed-text via Ollama)
- **Channels in use**: (e.g. Telegram, dashboard, CLI)

## Relevance filters

The scout uses these to decide whether a finding is worth reporting.

- **Discard if**: requires hardware you do not have (e.g. >24 GB VRAM if your card is 8 GB).
- **Discard if**: rehash of a tool already in your stack.
- **Discard if**: enterprise/cloud-only with no self-hosted path.
- **Watch list**: tools that look promising but are pre-1.0 or under-documented; revisit in 30 days.
- **Integrate now**: drop-in tools that match the stack, are mature, and have an obvious win.
