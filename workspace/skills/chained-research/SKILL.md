---
name: chained-research
description: Multi-model deep research using chained prompting (Gemini/Claude primary, DeepSeek critique, merge). Use for strategic decisions, competitive analysis, product positioning, technical tradeoff studies, or anywhere a single-model one-shot would miss blind spots. Mandatory DeepSeek red-team step to counter Western/American framing bias. Triggers on "deep research", "chain research", "red-team this", "what am I missing", "cross-check with DeepSeek", "multi-model analysis".
---

# Chained Research

**Scientific-calculator mindset.** Use the time a single model saves you to go DEEPER, not to finish faster. One page-and-a-half prompt + three model passes beats ten shallow searches.

## When to use

Use this skill when:
- Strategic decision or positioning question
- Competitive/market analysis
- Technical architecture tradeoff
- Anything where a one-shot answer could embed blind spots
- User says "deep research on X" or "red-team this"

**Do NOT use** for: simple factual lookups, quick code edits, status checks, or anything where speed matters more than depth.

## Workflow

### 1. Write the prompt properly
Target: **page-and-a-half prompt** (~500-800 words). Include:
- Context (who, what, current state)
- Constraints (budget, timeline, stack, values)
- Specific questions (3-5, numbered)
- Output format expectations
- Explicit "what do I not know that I should know?"

Short prompts get shallow answers. Don't skip this step.

### 2. Primary pass — Gemini or Claude
Pick one:
- **Gemini** (`gemini` skill) — good for broad synthesis, market data, multi-source grounding
- **Claude / this session** — good for reasoning chains, technical tradeoffs, nuance
- **Oracle** (`oracle` skill if present) — good for bundling project files into the context

Get a full answer. Save it to `tmp/research-<topic>-primary.md`.

### 3. Red-team pass — DeepSeek V4 (MANDATORY)
Feed the primary answer into DeepSeek for critique. **Prefer `--file` over pipes** — the exec sandbox blocks many pipe patterns.

```powershell
node tools/deepseek.js --critique --file tmp/research-<topic>-primary.md
```

This auto-routes to **deepseek-v4-pro with thinking enabled** (1.6T MoE, 1M context, $1.74/$3.48 per 1M tokens) — the heavy-lift critique. Released 2026-04-24, replaces the old R1 reasoner path. Roughly 7x cheaper than Sonnet/Opus for the same critique job.

For a faster, cheaper critique pass on smaller drafts, force flash-thinking:

```powershell
node tools/deepseek.js --model flash-thinking --critique --file tmp/research-<topic>-primary.md
```

Pipe form also works in plain terminals (not always in exec):

```powershell
Get-Content tmp/research-<topic>-primary.md | node tools/deepseek.js --critique
```

To save the critique to a file:

```powershell
node tools/deepseek.js --critique --file tmp/research-<topic>-primary.md | Out-File -Encoding utf8 tmp/research-<topic>-critique.md
```

DeepSeek's job is to:
- Surface Western/American framing bias
- Point out missing stakeholders, markets, or approaches
- Flag assumptions the primary model treated as given
- Suggest counter-positions

Save output to `tmp/research-<topic>-critique.md`.

### 4. Merge pass
Combine primary + critique into a final brief. You can do this yourself (you're good at synthesis) or hand both files to Claude/ChatGPT with a merge prompt:

> "Here are two takes on [topic]: [primary] and [critique]. Produce a single brief that integrates the critique's valid pushback into the primary analysis. Flag anything still uncertain."

Save final to `tmp/research-<topic>-final.md` (or wherever the user wants it).

### 5. Report
Deliver to the user:
- 3-5 sentence executive summary
- Top 3 actionable findings
- 1-2 biggest open questions
- Path to the full final brief

## Quality bar

- Primary prompt must be at least 400 words with numbered questions
- Critique step is non-optional, even when the primary answer looks great
- If DeepSeek returns <200 words of critique, your primary prompt was probably too narrow — rewrite and retry
- Always name the source model for each claim in the final brief

## Anti-patterns

- Skipping the critique because "the first answer was good" — you'll never catch your own blind spots
- Running three models on the same shallow prompt — the chain amplifies the prompt quality, garbage in = 3x garbage
- Using `web-search` instead of model-based research for strategic questions — web-search is for current facts, not synthesis
- Not saving intermediate files — you lose the audit trail and can't iterate

## Files

- `references/prompt-template.md` — reusable prompt skeleton for step 1
