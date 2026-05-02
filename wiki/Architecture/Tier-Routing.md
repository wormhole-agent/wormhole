# Tier Routing

Tier routing is the rule that picks which model gets which job. Cheap models for triage, deep models for reasoning, local models for offline fallback. The default lineup uses Anthropic Claude Opus for primary reasoning, OpenAI for general-purpose, DeepSeek V4-Pro for the critique gate, and Ollama for offline. This page covers the tier definitions, the `[brain].fallbacks` config, how a request walks the tier ladder, and how to add a new model or change the routing without rewriting the brain.

<!-- TODO: fill in during Phase 1 docs sprint -->
