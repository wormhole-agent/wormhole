# Provider Config Reference

Every supported LLM provider, the `[providers.*]` config block it expects, the env var that overrides the config, the default model, and the timeouts. Covered: Anthropic (Claude), OpenAI (GPT family), DeepSeek (via OpenAI-compatible adapter), Ollama (local). Includes the precedence order (env var, then config, then OpenClaw legacy locations) and the fallback chain `[brain].fallbacks` that the brain walks when a provider fails.

<!-- TODO: fill in during Phase 1 docs sprint -->
