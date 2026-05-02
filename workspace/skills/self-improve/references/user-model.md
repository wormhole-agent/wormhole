# User Model — template

> The self-improve skill builds up a structured model of the user over time. This file ships as a stub. The agent updates it after interactions that reveal preferences, decision patterns, or technical level.

Last updated: (set on first update)

## Communication Style

- (e.g. "prefers concise answers" [observed 5x])
- (e.g. "humor welcome, dry style" [observed 3x])

## Decision Patterns

- (e.g. "moves fast, decides fast")
- (e.g. "comfortable delegating, will say 'just do it'")
- (e.g. "risk-tolerant with experiments, conservative with money")

## Technical Level

- Strong: (areas where the user works fluently and does not need explanation)
- Comfortable: (areas where the user can follow without too much hand-holding)
- Learning: (areas where the user wants more context)

## Work Rhythm

- Active hours: (when the user typically works)
- Response-time expectation: (cron-okay vs sync-only)

## Correction Patterns

- (things the user repeatedly fixes; these are the preferences the model keeps getting wrong)

## Tool Preferences

- (which tools / approaches the user prefers over alternatives)

## Values

- (what the user optimizes for: cost, speed, quality, learning)

---

## How to update this file

- Only update on clear signal (explicit feedback or 3+ observations of the same pattern; not single observations).
- Label confidence: `[observed 3x]` versus `[inferred]`.
- Do not overwrite. Append with date. Let patterns emerge over time.
- Review during weekly memory maintenance.

The shape above is a starting point. If the user's life or work shifts, the section names can shift too.
