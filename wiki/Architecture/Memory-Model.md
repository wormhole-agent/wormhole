# Memory Model

Memory in WormHole is a layered set of markdown files. The fast layer is the daily file at `memory/YYYY-MM-DD.md`, capturing the current session. The slow layer is `MEMORY.md`, the long-term store. Between them sits `MEMORY-proposed.md`, a holding pen for items the dreaming pass thinks belong in long-term memory but a human has not yet promoted. This page explains the read path (what the agent sees on every turn), the write path (where session output lands), the dreaming consolidation pass, and the human-in-the-loop promotion gate that keeps `MEMORY.md` clean.

<!-- TODO: fill in during Phase 1 docs sprint -->
