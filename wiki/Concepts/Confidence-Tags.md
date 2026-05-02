# Confidence Tags

Confidence tags are the agent's way of saying "I know this" versus "I am guessing." Every claim the agent writes into memory is tagged: HIGH (verified or directly observed), MEDIUM (reasonable inference), LOW (a guess, kept around because it might be useful but should not be trusted). This page covers the tag taxonomy, how the agent decides which tag to apply, how downstream systems (dreaming, promotion gate, dashboard) read the tags, and how a human can override a tag when the agent gets it wrong.

<!-- TODO: fill in during Phase 1 docs sprint -->
