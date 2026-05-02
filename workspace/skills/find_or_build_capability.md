---
name: find_or_build_capability
description: Decision loop for when you need a capability (CLI / API / MCP server / skill) that isn't already wired up.
when_to_use: Whenever a task seems to require functionality you don't currently have. Apply BEFORE asking the user for help.
---

# Goal

Avoid asking the user "I can't do X — please install Y for me." Instead, follow this loop and *try* to acquire the capability yourself, then ask the user for confirmation if a step has real cost (admin install, paid API, irreversible action).

# Loop

1. **Already a skill?**
   - Call `list_skills`. If a skill matches the task, call `load_skill` with that name and follow it.

2. **Already a CLI on this machine?**
   - Use the `shell` tool. Quick checks:
     - `mcporter list` — see what MCP servers are registered (Notion, Slack, GH, etc.)
     - `gh --help`, `npm --help`, `pip --help`, `python --version`
     - `which <name>` / `where <name>` for a specific binary
   - If a fitting tool is already installed, *use* it (`shell` invocation), then surface the result.

3. **Find an existing tool/server?**
   - For CLIs: `npm search <keyword> --json | head -200`, `gh search repos <keyword> --limit 10 --json fullName,description,stargazerCount`.
   - For MCP servers: search `https://github.com/modelcontextprotocol` and `gh search repos "mcp-server-<topic>"`.
   - For APIs: prefer ones with stable v1 docs, free tier, no key needed for read.
   - Surface the top 1–3 candidates with a one-line "what + how to install + risk" summary. If the user has not yet authorised an install, **stop and ask** before doing one.

4. **Build it from scratch only if 1–3 fail.**
   - For one-shot scripts: write a small bash/python wrapper using the `write_file` tool into `skills/` (markdown skill) or `~/.openclaw/workspace/_scratch/` (throwaway script).
   - For anything bigger (multi-file project, persistent server): use `delegate_claude` with a tight scope and a clear acceptance test.
   - Always: leave a *skill* behind that documents how to invoke the new thing, so future-you finds it via `list_skills` next time.

5. **Register what you built.**
   - Drop a new `<name>.md` in `~/wormhole/skills/` so `load_skill` picks it up on next start. Format: front-matter (`name`, `description`, `when_to_use`), then a body that says exactly what shell command(s) to run.

# Hard rules

- **No silent admin installs.** `winget install`, `choco install`, `npm install -g`, `cargo install`, `pip install --user` for unknown packages all need a Telegram thumbs-up before running. Tell the user the package name + publisher + URL first.
- **No fetching code from the public internet and piping it to a shell.** The shell block-list refuses `curl ... | bash` for a reason.
- **Tools that touch money or send messages to other humans** (Stripe writes, Resend sends, Twitter posts, Telegram broadcasts to other chats) require explicit confirmation in the same turn, every time. Don't infer authorisation from a previous request.
- **If you're about to delete or overwrite something the user might want**, copy first: append `.bak-<date>` and keep the original.

# Telegram acknowledgement format

When you do need a green light, ask in this shape:

```
[capability check]
need: <one sentence — what you're trying to do>
candidate: <package / tool / API name + URL>
risk: <none / low / medium / high>
proceed?
```

Larry expects "yes" / "go" / a thumbs-up emoji as a green light, anything else as a hold.
