# memory/

Daily journal files for the agent. One file per day, named `YYYY-MM-DD.md`. The agent reads today's file (and usually yesterday's) at the start of every session and appends to today's file as work happens.

## What goes here

- A short note when the agent does something worth remembering: a fix, a decision, a surprising tool output, a bug it patched.
- Lessons that might earn promotion to `MEMORY.md` after the dreaming pass at 03:00 reviews them.
- Tags in square brackets to mark categories: `[auto-learn]`, `[surprise]`, `[tier-skip]`, `[v4-skip]`, `[module-name]`.

## What does NOT go here

- API keys, tokens, passwords. Hard rule. If a credential lands in a daily memory file, treat it as a leak: rotate the key, scrub the file, announce.
- Live customer data. The agent should never paste a customer's email, address, or transcript into a file inside the workspace. Anonymize or summarize.

## File format

Plain markdown. Date headers if the file has multiple sessions. Otherwise just chronological notes.

```
# 2026-05-02

## Morning

- Pulled the latest config; the cron job for X was failing because Y. Fixed.
- [auto-learn] When the watchdog is restarted, the cron-state file resets. Add a check to preserve last_run_ts on boot.
- [example-module] Wrote a draft of the new feature spec. Sent it through the chained-research skill.

## Evening

- Reviewed dreaming proposals; promoted two lessons to MEMORY.md.
```

## .dreams/ subdirectory

The dreaming pipeline writes intermediate state into `memory/.dreams/`. That directory is gitignored. Do not commit it.

## What the repo ships

Zero daily memory files. They are per-user, per-day, and frequently contain personal or project-specific content. The pattern is documented here; the content is yours.
