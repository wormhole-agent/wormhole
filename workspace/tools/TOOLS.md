# TOOLS.md

> The agent should look for, find, and build her own tools before asking the user for help. This file lists the ten tools that ship with v0.1.0. The list is the floor, not the ceiling. Add to it.

## Find or build

The default rule from `AGENTS.md` is: when the agent needs a capability,

1. Check `TOOLS.md` (this file) and the `tools/` directory.
2. Check installed skills (`workspace/skills/`) and module-scoped skills (`modules/<name>/skills/`).
3. Check installed MCP servers, if any.
4. If a tool exists, use it. If a tool almost exists, fix or extend it. If no tool exists and the task is repeatable, build the tool.
5. The user is the last resort, not the first.

The toolkit grows. Tools can be one Node script. Skills can be one markdown file. Both are short and readable. The barrier to adding one is meant to be low.

## The ten that ship

### 1. web-search.js — Multi-source web search

```
node tools/web-search.js "query"
node tools/web-search.js --count 5 --provider brave "query"
node tools/web-search.js --json "query"
```

Providers tried in order: Brave (needs `BRAVE_SEARCH_API_KEY`), DuckDuckGo, SearXNG. The DDG path needs no API key, so a fresh-install user gets value on day one. Brave's free tier is roughly $5/month of credits and a cleaner index; sign up at `https://api-dashboard.search.brave.com/register`.

### 2. deepseek.js — DeepSeek wrapper

```
node tools/deepseek.js "prompt"                            # default = v4-flash
node tools/deepseek.js --model pro "harder problem"        # v4-pro
node tools/deepseek.js --model pro-thinking "deeper still" # v4-pro with reasoning
node tools/deepseek.js --critique < draft.md               # auto-routes to pro-thinking
node tools/deepseek.js --json "prompt"
```

The DeepSeek V4 family covers a cheap general model (`flash`), a heavyweight reasoner (`pro`), and reasoning-mode variants. Context: 1M tokens on V4. Output: 384K. License: MIT. Get a key at `https://platform.deepseek.com/`. Set `DEEPSEEK_API_KEY` in your env or vault.

The `--critique` flag is the project's red-team gate. Pipe a draft in, get back a structured critique. Pairs with `skills/chained-research`.

### 3. yt-transcript.js — YouTube transcript fetcher

```
node tools/yt-transcript.js <url>             # print transcript to stdout
node tools/yt-transcript.js --save <url>      # save to logs/yt-transcripts/<id>.txt
```

Shells out to `yt-dlp` (you install separately: `pip install yt-dlp`). Pulls subtitles when available, falls back to auto-generated captions. The "always read transcripts before commenting" rule from `AGENTS.md` lives or dies on this tool.

### 4. deep-dive.js — Long-context module dump

```
node tools/deep-dive.js <module> "question" [--provider sonnet|opus|gemini|deepseek-pro] [--include-memory] [--dry-run] [--out file.md]
node tools/deep-dive.js --path ./some/dir "question"
```

Loads an entire directory into one long-context LLM turn for cross-file synthesis. Always run `--dry-run` first to see token count and estimated cost; long-context dumps to top-tier models can be expensive. Defaults to Sonnet (200K). Use `--provider gemini` for very large dumps (2M context). DeepSeek Pro at 1M context is the cheapest long-context tier in 2026.

### 5. telegram-send.js — Cron-safe Telegram outbound

```
node tools/telegram-send.js "Your message"
node tools/telegram-send.js --silent "No-ping message"
echo "long body" | node tools/telegram-send.js --stdin
node tools/telegram-send.js --chat <chat-id> --reply-to <msg-id> "Reply text"
```

Programmatic: `const { send } = require('./tools/telegram-send'); await send('Hello');`. Reads bot creds from the DPAPI vault via `secrets/accessor.js`. Cron-safe: never throws unhandled exceptions, never blocks longer than 10 seconds, truncates at 4096 chars. Configure `TELEGRAM_BOT_TOKEN` and `TELEGRAM_DEFAULT_CHAT_ID` in env or vault.

### 6. web-audit.js — Static-site SEO + ADA + AI-readability check

```
node tools/web-audit.js <site-dir>            # audit one site
node tools/web-audit.js <site-dir> --json     # machine-readable
node tools/web-audit.js <site-dir> --fix-meta # add scaffolds for robots.txt, sitemap, llms.txt
```

Heuristic, not a full WCAG conformance test. Catches the easy misses: missing schema, no `llms.txt`, no alt text, no `h1`, no canonical, missing JSON-LD. Use Lighthouse, axe, and the schema.org validator for the deep checks.

### 7. screenshot_and_send.js — Headless-Chrome screenshot

```
node tools/screenshot_and_send.js <url>
```

Takes a screenshot of a URL via headless Chrome and sends it to the configured default Telegram chat. The "verify live deploys" rule from `AGENTS.md` lives or dies on this tool.

### 8. triage-router.js — Small-model classifier

```
ollama run qwen2.5:1.5b "<prompt>"   # raw model
node tools/triage-router.js classify "is this code or prose?"
```

Uses a small local Ollama model (default: `qwen2.5:1.5b`, ~1 GB) to do triage classifications before reaching for a top-tier model. Yes/no routing, intent classification, simple extraction. Saves 20 to 30 percent of top-tier calls in practice. Requires Ollama (see GETTING-STARTED.md Step 3).

### 9. toon.js — Token-Oriented Object Notation

```
node tools/toon.js encode <json-file>     # compress structured output
node tools/toon.js decode <toon-file>     # decompress back to JSON
node tools/toon.js compare <json-file>    # show token savings estimate
```

Compact JSON alternative for structured LLM payloads. 30 to 60 percent token reduction on typical structured outputs; lossless on non-null content. Use when sending big structured payloads through an LLM context.

### 10. compact-session.js — Session transcript compaction

```
node tools/compact-session.js sessions/today.jsonl
node tools/compact-session.js sessions/today.jsonl --provider ollama --model gemma4:e4b
```

Compresses old turns of a session JSONL into a summary turn so the agent can keep context without paying for the full transcript every turn. Uses a local Ollama model by default (free). Output overwrites the input by default; `--out <path>` to write elsewhere.

## Secrets infrastructure (not tools, but ships in tools/)

- `secrets-vault.js` — DPAPI vault: `status`, `lock`, `unlock`, `verify`, `edit`. Windows-only.
- `secrets/accessor.js` — runtime credential reader. Other tools call `getByLabel(label, group)` here. Do NOT hardcode secrets anywhere.
- `secrets/vault.js` — the vault primitive (encryption + decryption).
- `secrets/registry.js` — secret-id and label registry.
- `secrets-list.js` — show what is in the vault without decrypting values.
- `secrets-guard.js` — pre-commit and pre-cron payload scanner. Mandatory in your pre-commit hook.

These are infrastructure. The hard rule is: API keys, tokens, bearer values, and passwords live ONLY in the DPAPI-encrypted vault (`secrets.md.enc`). Never in memory files, never in cron payloads, never in any git-tracked file. If a key shows up in a git-tracked file, treat it as a live leak.

## Additional tools (install on demand or build your own)

These exist in the maintainer's working tree but do NOT ship with v0.1.0. If you want one, port it yourself or wait for a Phase 2 release that bundles more:

- `puter-search.js` — Puter.com Perplexity wrapper. Requires a Puter account. Per Lock 2 (2026-05-02), Puter is not a project dependency.
- `accountant.js` — Stripe revenue reader.
- `librarian.js` — workspace organizer; rule set is opinionated.
- `system-scorecard.js` — longitudinal "is the system getting better" metric. Tuned to the maintainer's harness.
- `cf-zone-inspect.js`, `cf-zones-list.js` — Cloudflare zone tooling.
- `gmail-reader.js`, `biz-email-watcher.js` — email integrations.
- `wave-orchestrator.js` — multi-agent orchestrator.

The find-or-build principle applies here too. If you need any of these, write your own; the shape is short and copyable.

## Adding a new tool

1. Drop a file at `workspace/tools/<your-tool>.js`.
2. Start with a 2-3 line docstring (the readability standard treats this as required).
3. Use Node 20 built-ins where possible (`fetch`, `fs/promises`, `child_process`).
4. If you need a credential, read it via `tools/secrets/accessor.js`. Never hardcode.
5. Add a row to this file under the right section.
6. Commit. Push. Done.

If the tool is reusable across projects, that is the floor. If it does something subtle, write a short skill in `workspace/skills/` that documents WHEN to use the tool. The agent reads the skill; the skill calls the tool.
