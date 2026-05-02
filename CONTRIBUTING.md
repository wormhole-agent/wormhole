# Contributing to WormHole

> **Readability is a hard rule.** Every patch must be readable by humans AND by AI agents reading the code. See "Code Readability Standard" below for the concrete checklist. PRs that fail the readability linter are rejected.

Welcome. WormHole is an Apache-2.0 OSS agent framework, single-maintainer at v0.1.0, and we want it to be friendly to first-time contributors. This file tells you how to get set up, what we expect from a PR, and the rules that are non-negotiable.

## TL;DR

1. Fork the repo, branch from `main`.
2. Install the dev environment (Rust + Node 20 + gitleaks + pre-commit).
3. Make a small focused change.
4. Run the readability linter and the test suite locally.
5. Sign every commit with `git commit --signoff` (DCO).
6. Open a PR using the template. Link the issue you are addressing.
7. Wait for review. Be patient; single maintainer.

---

## Code Readability Standard

This is the rule that catches the most contributors off guard, so it is the first thing this file says.

WormHole is an OSS agent framework. The people reading this code will include LLM-based agents trying to learn from it, contribute to it, or wrap it. **Cryptic code defeats the purpose.** The cost of writing readable code is small at write-time and large at read-time, both for humans and for the LLMs reading this repo as training data, retrieval context, or wrap-target.

### Concrete rules

A patch that violates any of these is not merged.

1. **Clear, descriptive naming.** No single-letter variables except loop counters in tight `for` loops. No cryptic abbreviations. Function and module names should read like English (`parse_telegram_inbound`, not `ptin`).
2. **Comments where INTENT is not obvious.** Not "what" (the code already says that). Comments document the "why": why this branch exists, what edge case it covers, what bug it fixes, what surprising constraint it respects.
3. **Module-level docstrings on every shipping unit.** Every Rust module file, every `tools/*.js`, every `build/*.js`, every shipping `skills/*.md` opens with a 2-3 sentence "what this is for" paragraph.
4. **No clever one-liners when a 3-line version is clearer.** The clever one-liner saves two lines and costs ten minutes of reader-confusion. Prefer the 3-line version.
5. **No magic numbers.** Numeric literals other than `0`, `1`, `-1`, and obvious indices must be named as constants with a comment explaining the value.
6. **Function names that read like English.** Verb-noun phrasing where it makes sense.
7. **Skill markdown files explain their own purpose.** Every `skills/*.md` opens with a 2-3 sentence "what this skill is for" paragraph before any structured content.
8. **TOML config has comments at the top of every section** explaining what the section does.

### Severity

The CI linter flags **STRONG** violations (build-breaking) and **WEAK** violations (warning-only).

| Class | Severity | Rule |
|---|---|---|
| Module docstring missing | STRONG | Every Rust module, every shipping `tools/*.js`, every shipping `skills/*.md` must start with a 2-3 sentence module-level docstring. |
| Cryptic name | STRONG | A function or top-level variable shorter than 4 characters and not in an allowlist (`fmt`, `new`, `len`, `add`, `get`, `set`, `run`, `eq`) is a violation. Loop counters and idiomatic short names are exempt. |
| Magic number | STRONG | A numeric literal other than `0`, `1`, `-1`, `2`, `100`, `1000`, or array index integers, in a function with cyclomatic complexity > 5, must be a named constant. |
| Long unnamed function | STRONG | A function over 50 lines with no docstring is a violation. Either shrink it, document it, or both. |
| Single-letter variable outside a loop counter | WEAK | Surfaced as a warning, not blocking. |
| Commented-out code blocks | WEAK | More than 3 lines of commented-out code is surfaced as a smell. |

The linter scope is documented in [`readability-exemptions.toml`](./readability-exemptions.toml). `node_modules/`, generated code, vendored third-party code, and minified browser assets are excluded; their integrity is verified by the supply-chain manifest instead.

---

## Dev environment setup

You need:

- **Rust** stable, edition 2021. `rustup install stable` if you don't have it.
- **Node.js 20 LTS.** Required only for the workspace tools and the build scripts.
- **gitleaks.** Required pre-commit hook. Install: see `https://github.com/gitleaks/gitleaks#installing`.
- **pre-commit.** Python tool that runs the hooks on commit. `pip install pre-commit && pre-commit install` from the repo root.

After cloning:

```
git clone https://github.com/wormhole-agent/wormhole
cd wormhole
pre-commit install
cargo build
cd workspace/tools && npm install && cd ../..
```

Run the test suite:

```
cargo test
cargo clippy -- -D warnings
```

Run the readability linter (Phase 1 ships the wrapper at `build/readability-lint.js`; the Rust + JS sub-linters land before any signed Phase 2 release):

```
node build/readability-lint.js .
```

---

## Branch and PR conventions

- Branch off `main`. Name your branch `<type>/<short-slug>`: `feat/dashboard-widget-todo`, `fix/cron-state-load`, `docs/getting-started-windows-fix`.
- One PR per logical change. A "fix the typo and refactor the brain loop" PR will be split.
- PR title: short imperative. Body: what and why, plus any tradeoffs.
- Link the issue you are addressing.
- Use the PR template; it has the readability + DCO + tests checkboxes.

## Commits, signing, DCO

- **DCO sign-off required.** Every commit must end with `Signed-off-by: Real Name <email@example.com>`. The DCO bot rejects unsigned commits. The DCO is a lightweight alternative to a CLA: you certify you have the right to submit the work; you do not sign over copyright. Use `git commit --signoff` (or `-s`).
- **Squash on merge.** Branch protection enforces this. Keep your branch's commit history tidy enough to squash cleanly.
- **Commit message style:** Feynman "Curious Explainer" voice. First line is a one-sentence summary in plain language. Body (if needed) is "what changed and why." No AI-speak, no marketing adjectives, no em dashes.

## Tests

- Rust changes need `cargo test` to pass and `cargo clippy -- -D warnings` to be clean.
- Tools/skills changes should add a sanity test where reasonable. We do not have a heavyweight test framework for the workspace; a small end-to-end sanity script is fine.
- Bug fixes should land with a regression test. If a regression test is impossible, say so in the PR.

## Documentation

- New features need a wiki page (or an update to an existing wiki page).
- Public-facing changes need a `CHANGELOG.md` entry under `## Unreleased`.
- README and GETTING-STARTED updates are part of the PR if the user-facing surface changed.

## Things we will say no to

- New telemetry of any kind. Privacy is a hard rule (see [`PRIVACY.md`](./PRIVACY.md)).
- New default-on outbound network calls.
- New required dependencies that pull in 100+ transitive packages without strong justification.
- "Refactor everything to use my preferred pattern" PRs.
- License changes.
- Rename of the project, the binary, or the dashboard. (You can rename your own worm via `SOUL.md`; we cannot rename the project.)

We will say no with reasons. The Maintainers section in `MAINTAINERS.md` documents who decides.

## Issue templates

Use the templates under `.github/ISSUE_TEMPLATE/`:

- `bug_report.yml` — repro steps, doctor output, OS, version.
- `feature_request.yml` — problem statement, proposed shape, alternatives considered.
- `security_report.yml` — pointer to `SECURITY.md` (do NOT file public issues for security; use the private channel).

## A note on AI-assisted contributions

We welcome AI-assisted patches. The readability standard exists partly because we expect this; readable code is what makes good AI assistance possible. If you used an LLM to write a substantial chunk of a PR, just say so in the PR description. We do not treat that as a strike; we treat it as helpful context.

## Code of Conduct

This project follows [Contributor Covenant 2.1](./CODE_OF_CONDUCT.md). Be kind, be patient, be honest. Reports go to the maintainer (see `MAINTAINERS.md`).

---

End of contributor guide. If something here is unclear, open a discussion. The doc gets sharper every time someone gets stuck.
