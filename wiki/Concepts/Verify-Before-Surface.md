# Verify Before Surface

Verify-before-surface is the rule that says the agent does not show a result to the user until it has at least one verification step. If a tool says a file exists, the agent reads the file. If a search says a fact is true, the agent finds a second source. The cost is a little extra work; the payoff is that the user can trust what the agent reports. This page explains the rule, the verification patterns the codebase already uses, and the cases where verification is impractical and the agent has to admit it is guessing.

<!-- TODO: fill in during Phase 1 docs sprint -->
