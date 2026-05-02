---
name: self-improve
description: Self-improvement loop for OpenClaw. Triggers automatically after complex multi-step task completions to extract reusable skills, update user modeling, and refine existing skills based on corrections. Also runs on heartbeat to promote patterns from daily memory into long-term knowledge. Use when a complex task just finished, when the user corrects an approach, when asked to "learn from this", or during memory maintenance heartbeats.
---

# Self-Improve

Closed learning loop that makes OpenClaw smarter over time. Three mechanisms:

## 1. Auto-Skill Extraction

After completing a complex task (3+ tool calls, multi-step reasoning, or novel approach), evaluate whether the approach is reusable.

### Trigger Criteria
A task qualifies for skill extraction if ANY of:
- Used 3+ different tools in sequence
- Required a non-obvious workaround
- User said "nice", "perfect", "that worked" or similar positive signal after a hard task
- The approach would save significant time if repeated

### Extraction Process
1. Identify the reusable pattern (not the specific data)
2. Check `skills/` directory — does a similar skill already exist?
3. If new: create a minimal skill using the skill-creator flow
4. If exists: update the existing skill's SKILL.md with the new approach
5. Log the extraction in today's `memory/YYYY-MM-DD.md`

### What Makes a Good Auto-Skill
- **Procedural** — a sequence of steps, not just knowledge
- **Reusable** — applies to a class of problems, not one instance
- **Non-obvious** — the agent wouldn't naturally do it without the skill

### Anti-Patterns (don't extract)
- Simple one-step tasks
- Highly specific to one context (e.g., "fix this user's specific Paperclip migration")
- Already covered by an existing skill
- Common knowledge any LLM would know

## 2. User Model Updates

Beyond flat memory, maintain a structured understanding of the user in `references/user-model.md`. Update after interactions that reveal:

### Dimensions to Track
- **Communication style** — verbosity preference, humor tolerance, emoji usage
- **Decision patterns** — risk tolerance, speed vs. thoroughness, delegation comfort
- **Technical level** — what they understand without explanation
- **Work rhythm** — when they're active, response time expectations
- **Correction patterns** — what they commonly fix (these reveal preferences the model keeps getting wrong)
- **Tool preferences** — which tools/approaches they prefer over alternatives
- **Values** — what they optimize for (cost, speed, quality, learning)

### Update Rules
- Only update on clear signal (explicit feedback or repeated pattern, not single observations)
- Label confidence: `[observed 3x]` vs `[inferred]`
- Never overwrite — append with date, let patterns emerge
- Review during weekly memory maintenance

## 3. Skill Refinement Loop

When an existing skill is used and the user corrects the output:

1. Identify what was wrong (approach, format, tool choice, assumption)
2. Read the skill's SKILL.md
3. Add a "lesson learned" or adjust the procedure
4. Log the refinement in daily memory

### Refinement Triggers
- User says "no, do it like X instead"
- User manually fixes output after skill was followed
- Skill produces an error that requires recovery
- User explicitly says "remember this for next time"

## 4. Memory Promotion (Heartbeat)

During memory maintenance (heartbeat or explicit request):

1. Scan recent `memory/YYYY-MM-DD.md` files (last 7 days)
2. Identify patterns that repeat across days:
   - Same type of task done multiple times → candidate for auto-skill
   - Same correction made repeatedly → update user model
   - Temporary workaround that stuck → promote to permanent knowledge
3. Promote to MEMORY.md or create/update skills accordingly
4. Archive stale entries (>30 days, no longer relevant)

## Integration

This skill is passive — it doesn't need to be explicitly invoked. Instead:

- **After complex tasks**: Check trigger criteria, extract if qualified
- **On user corrections**: Update skill or user model
- **On heartbeat memory maintenance**: Run promotion loop
- **On explicit "learn from this"**: Extract whatever the user points at

Keep extractions lightweight. A bad auto-skill wastes context on every future invocation. When in doubt, log the pattern in memory and wait for a second occurrence before creating a skill.
