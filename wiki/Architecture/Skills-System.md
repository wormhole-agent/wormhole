# Skills System

A skill is a markdown file the agent loads on demand. The agent decides "this looks like a research task; load the chained-research skill," then reads the file and follows the instructions inside it. Skills are not code; they are prompts with structure. This page explains the skill format, the front-matter conventions, the `load_skill` tool that triggers a load, the curation loop where the agent itself proposes new skills, and the line between skills (markdown) and tools (Node code).

<!-- TODO: fill in during Phase 1 docs sprint -->
