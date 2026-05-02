<!-- Thanks for sending a PR. The checklist below is short on purpose. -->

## What this changes

<!-- One paragraph. What does the patch do? -->

## Why

<!-- One paragraph. What problem does it solve, what use case did you have? -->

## How it was tested

<!-- "I ran cargo test", "I ran the binary against my own workspace and the dashboard widget rendered", etc. -->

## Checklist

- [ ] Branch named `<type>/<slug>` (e.g. `feat/dashboard-widget-todo`).
- [ ] Every commit signed off (DCO): `git commit --signoff`.
- [ ] `cargo test` passes locally.
- [ ] `cargo clippy -- -D warnings` is clean.
- [ ] Readability linter passes (`node build/readability-lint.js .` once it lands).
- [ ] No secrets, no `.token`, no provider keys in the diff. (gitleaks will catch this; double-check.)
- [ ] No new telemetry. (See [`PRIVACY.md`](../PRIVACY.md). This is a hard rule.)
- [ ] No new default-on outbound network calls.
- [ ] CHANGELOG entry added under `## Unreleased` (for user-facing changes).
- [ ] Wiki page updated or added (for new features).

## AI-assistance disclosure (optional)

<!-- We welcome AI-assisted patches. If a substantial chunk of this PR was
written with LLM help, mention it here. Not a strike; helpful context. -->
