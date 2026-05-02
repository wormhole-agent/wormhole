# Support

Best-effort. No SLA. Single maintainer. Honest expectations below.

## Where to get help

- **GitHub Discussions** (`https://github.com/wormhole-agent/wormhole/discussions`): the right place for "how do I do X?" and "is this supposed to work?" Search before posting; an answer may already exist.
- **GitHub Issues** (`https://github.com/wormhole-agent/wormhole/issues`): bug reports, feature requests, and concrete reproducible problems. Use the issue templates.
- **Wiki** (`https://github.com/wormhole-agent/wormhole/wiki`): the architecture deep-dives, schema references, and operations runbooks. The Operations -> Troubleshooting page has the canned recovery procedures.
- **`wormhole doctor`**: the first stop for any "something broke" question. By default it makes zero outbound network calls; it checks the local install (binary, vault, config, ports, scheduled task, logs). Run it and read its output before opening an issue.

## Where NOT to go for help

- **Email the maintainer.** We do not have a support inbox. The exception is `security@wormhole-agent.dev` for security issues only (see [`SECURITY.md`](./SECURITY.md)).
- **Discord, Slack, or any chat.** None official. If you find a community Discord, it is a community Discord, not the project.
- **DM the maintainer on social media.** Same answer.

## What you can expect

- **Issue response time:** best-effort. Some weeks are fast. Some are slow. The maintainer is single-person, the project is OSS, and there is no on-call rotation.
- **Bug fixes:** prioritized by severity (data loss, security, build breaks first; UX paper cuts second; new features last).
- **Feature requests:** triaged. We say no a lot. Saying no to a feature is not a snub; it keeps the project shippable.
- **PRs:** read seriously. We reply within a couple weeks at most. Small, focused PRs land faster than large ones.

## What we ask of you

- **Read the wiki and the docs first.** A non-trivial fraction of "how do I do X?" is already answered there.
- **Run `wormhole doctor` first.** The output is often the answer.
- **Reproduce on a clean install** before reporting. "It works on my machine" cuts both ways.
- **Be patient.** Single-maintainer projects have human-shaped throughput.
- **Be specific.** A reproducible bug report with version, OS, doctor output, and steps gets fixed. A vague "it doesn't work" sits in triage.

## What we are NOT

- Not a paid product. There is no "support tier."
- Not a vendor. There is no contract.
- Not an enterprise tool. If you need an enterprise tool, you need an enterprise tool. WormHole is a personal AI agent that runs on one box.
- Not a managed service. We do not run anything for you.

## Commercial use

Apache-2.0 lets you do whatever you want, including running this in a commercial setting. Doing so does not entitle you to any support. If you want commercial support, you would need to pay someone (the maintainer is open to consulting conversations; reach out via GitHub).

## A final note

The fastest way to get a thing fixed is often to send a PR with the fix. We are friendly to first-time contributors. The CONTRIBUTING.md and the wiki's Community section will get you started.
