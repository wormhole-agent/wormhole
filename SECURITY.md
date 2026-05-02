# Security

> **Important:** Do NOT file public GitHub issues for security problems. Use the private channel below.

WormHole is an early-stage, single-maintainer OSS project. The security posture below is honest, not aspirational: best-effort response, no SLA, but the channel works and reports are taken seriously.

## How to report a security issue

### GitHub Security Advisory (private)

1. Go to `https://github.com/wormhole-agent/wormhole/security/advisories/new`.
2. Fill out the form. Mark severity honestly (low / medium / high / critical).
3. The advisory is private until we publish it. We coordinate the patch with you in the same advisory.

GitHub handles disclosure mechanics, CVE coordination if relevant, and gives us a private discussion thread. This is the only supported reporting channel for v0.1.0.

A dedicated `security@` mailbox may be added in a future release; until it is, do not send security reports to any other email or social channel — they will be missed.

## What is in scope

- The `wormhole` binary (the Rust source under `wormhole/`).
- The embedded dashboard (the routes and HTML served from `wormhole.exe`).
- The default workspace skills and tools that ship with this repo (`workspace/skills/`, `workspace/tools/`).
- The build and release pipeline (`.github/workflows/`).
- The Phase 2 MSI installer and first-run wizard (when those land).

## What is out of scope

- Bugs in upstream dependencies. Report those upstream and CC us so we can pull a patched version.
- Bugs in third-party tools that the user installed alongside WormHole (Ollama, age, Node, etc.). Report those upstream.
- Bugs in user-written tools dropped into a private fork.
- Issues that require already having admin on the user's machine.
- Issues that require already having the user's `.token` value (the token IS an authentication boundary; if you have it, you have access).

## What you can expect from us

- **Acknowledgement** within 7 days of report.
- **Initial triage** within 14 days.
- **Patch or remediation plan** as fast as is honest. Single-maintainer; some weeks are slow. We will not lie about timelines.
- **Coordinated disclosure.** If you give us a reasonable window before publicizing, we will use it well. If we are not making progress, escalate (bump the advisory).
- **Credit.** If you want it, you get a named line in the security advisory and in `CREDITS.md` Tier 3.

## What we ask from you

- **Give us a window.** A few weeks for low / medium issues. Longer for critical issues that affect users in the wild. We will tell you if we need more time and why.
- **Do not exploit the issue beyond what is needed to demonstrate it.** No data exfiltration, no DOS, no scrubbing your tracks.
- **Do not test against other people's machines.** Test against your own install.

## Severity guidance (for your report)

Use these as a sanity check; we will adjust if we disagree:

- **Critical:** remote unauthenticated code execution; vault key recovery without DPAPI access; release-manifest signature bypass.
- **High:** authenticated `/api/*` access without the `.token`; loopback bind escape (LAN exposure of dashboard); update channel hijack.
- **Medium:** local privilege bypass that requires already being on the box; logging of secret material; missing CSRF / Origin protection on dashboard.
- **Low:** typos in security docs, missing CSP headers, etc.

## Past advisories

None yet. v0.1.0 is the first public release.

## Why no SLA

We are an early-stage OSS project with one maintainer. Any SLA we wrote down would be a lie. The "best-effort, single-maintainer" posture is honest. If you need an SLA for a production deployment, you need a vendor; this project is not that.

## A note on shipping unsigned binaries

v0.1.0 ships unsigned binaries. The release-manifest is still Ed25519-signed (separate from Authenticode); update verification works. The unsigned-binary risk is "Windows SmartScreen will warn the user"; the user clicks through and the binary runs. Phase 2 adds Authenticode signing when adoption justifies the cert.

Reporting a finding tied to this trade-off (e.g. "an attacker could replace the binary on a download mirror") is in scope; we will discuss whether the threat model warrants accelerating Phase 2.
