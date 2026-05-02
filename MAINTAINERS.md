# Maintainers

The people responsible for the project. v0.1.0 is single-maintainer. As the project grows, additional maintainers will be listed here with their areas.

## Project lead

**TODO before public push:** insert real name, GitHub handle, and primary contact. The Phase 1 build left this as a placeholder.

- Name: Simon L. Paige
- GitHub: `@simonlpaige`
- Areas: everything (single maintainer)
- Time zone: CDT
- Best-effort response window: 7 days for issues, 14 days for PRs (see [`SUPPORT.md`](./SUPPORT.md))

## Code of Conduct enforcement

The project lead is the Code of Conduct point of contact until additional maintainers are added. Reports go to the same channel as security: `security@wormhole-agent.dev` (placeholder; see [`SECURITY.md`](./SECURITY.md)) or via a GitHub Security Advisory if the report is sensitive.

When a second maintainer joins, this section names two contacts so reports about one maintainer can go to the other.

## Security incident response

The project lead is the security incident point of contact. See [`SECURITY.md`](./SECURITY.md) for the disclosure flow.

## Release authority

The project lead has the only push key for `main`, the only signing key for the Ed25519 release manifest, and (when Phase 2 lands) the only access to the OV / EV code-signing cert.

Release-manifest key rotation requires a release signed by both the old and new key. This is documented in the build guide; it is a hard rule that prevents a single compromised key from hijacking the update channel.

## How to join

WormHole has a low bar for adding contributors and a higher bar for adding maintainers. The path looks like:

1. Land 5 to 10 substantive PRs over a few months.
2. Show up consistently in code review.
3. Demonstrate judgment about what to say no to (saying no is the maintainer skill).
4. The project lead invites; the new maintainer is added to this file.

There is no formal voting process at this size. When the project crosses ~5 active maintainers, we will write one.

## Decision process

For now: the project lead decides. Disagreements that escalate get worked out in the relevant issue or PR thread. Major architectural changes go through an RFC (filed as a discussion in GitHub Discussions, then turned into an issue, then implemented).

The lock list (the things that cannot change without a wider discussion):

- No telemetry, ever (see [`PRIVACY.md`](./PRIVACY.md)).
- Apache-2.0 license (see [`LICENSE`](./LICENSE)).
- Local-first architecture (no server-side tenancy, no SaaS).
- The "agent looks for and builds her own tools before asking the user" operating principle.
- Code Readability Standard (see [`CONTRIBUTING.md`](./CONTRIBUTING.md)).

A change to any of those requires a public RFC, not just a maintainer PR.
