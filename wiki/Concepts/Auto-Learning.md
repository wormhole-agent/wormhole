# Auto-Learning

Auto-learning is the loop where the agent improves its own skill set over time. The `self-improve` skill watches successful patterns, proposes new skills or refinements to existing skills, and writes them to a holding pen for human review. Once approved, the new skill is promoted to the curated set and the agent uses it on future turns. This page explains the loop, the proposal format, the human-review gate, and the safeguards that prevent the agent from rewriting itself in a way that breaks something important.

<!-- TODO: fill in during Phase 1 docs sprint -->
