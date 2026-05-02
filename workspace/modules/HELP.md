# Quick reference: modules

> The agent reads this when the user types `help!` (per `AGENTS.md`).

## What is a module

A module is a directory under `workspace/modules/` that holds the context for one project, one client, one ongoing piece of work. The point is to keep `MEMORY.md` from blowing up: project-specific facts live in the module, not in long-term memory.

A module looks like this:

```
modules/<name>/
  CONTEXT.md     # the meat: who, what, why, current state, open loops
  README.md      # short intro, mostly for humans
  LEDGER.md      # optional: open-loop tracker
  skills/        # optional: skills specific to this module
  tools/         # optional: tools specific to this module
```

## Activation

The user names a module by callword: "bring up <module>" or "switch to <module>". The agent reads that module's `CONTEXT.md` and starts working inside it. The agent tags daily memory entries with `[<module>]` while the module is active.

Multiple modules can be active at once: "bring up <a> and <b>" loads both contexts.

## Deactivation

"Go base" or "clear modules" drops module context. The agent returns to base mode (only `SOUL.md`, `USER.md`, `MEMORY.md`, today's daily file).

## Module index

`INDEX.md` lists every module's callword, path, and one-line description. Keep it tidy; the agent reads it to find which module to load.

## When to create a new module

When you have:

- A project that produces enough notes to clutter `MEMORY.md`.
- A client or customer with their own context (preferences, agreements, boundaries).
- A research thread that wants its own ledger and its own skills.

When in doubt, copy `example-module/` and rename it. You can always merge it back if it turns out to be over-organized.
