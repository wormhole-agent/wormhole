# V4 Critique Gate

For high-stakes writes (memory promotion, public-doc drafting, security-sensitive output) WormHole runs the candidate through a separate critique pass on a different model, typically DeepSeek V4-Pro. The producer makes the move; the critic looks for failure modes the producer missed. Only output that survives both passes is surfaced. This page explains why two models beat one bigger model for this case, how the gate is wired in `tools/deepseek.js`, when to invoke it, and when a single-model pass is good enough.

<!-- TODO: fill in during Phase 1 docs sprint -->
