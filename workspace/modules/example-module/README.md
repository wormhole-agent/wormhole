# example-module

> A module is a folder of project-specific context. This one is the reference shape; copy it when you start a real module.

## Files in this directory

- `CONTEXT.md`: the meat. Who, what, why, current state, open loops. The agent reads this on activation.
- `README.md`: this file. Short human-facing intro. The agent does not have to read it.
- `LEDGER.md`: open-loop tracker. Optional but useful.
- `skills/`: skills specific to this module. Optional.
- `tools/`: tools specific to this module. Optional.

## How to use

1. Copy this directory to a new name: `cp -r example-module my-project`.
2. Rename the callword in `INDEX.md` (one entry up).
3. Fill in `CONTEXT.md` with the project's facts.
4. Add a row to `modules/INDEX.md` so the agent can find it by callword.
5. From any session, say "bring up my-project" and the agent loads the context.

## When to delete a module

When the work is done and the context is no longer load-bearing, move the module to `modules/_archive/<name>/`. Keep the files; just take them out of the active set.
