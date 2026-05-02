---
name: example-skill
description: A stub skill showing the format. Copy this file, rename it, fill it in.
---

# example-skill

This is the skill format. The front-matter at the top has two keys: `name` (slug) and `description` (one-liner the skill loader shows in lists).

## What this skill does

(one paragraph: when should the agent reach for this, what does it do, what is the expected output)

## When to use it

(situations where this skill is the right answer)

## Steps

1. (numbered steps the agent runs)
2. ...

## Examples

(short examples of input and output)

## Notes

- Skills are markdown. They are NOT executed; they are read by the agent as context. The agent decides which tool calls to make based on the steps.
- Keep skills focused. One skill per discrete capability.
- If a skill grows past 200 lines, it probably wants to be two skills.
