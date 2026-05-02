---
name: ai-scout
description: Monitor the open-source AI community (Reddit, HN, GitHub Trending) for new tools, models, techniques, and breakthroughs relevant to the user's local AI stack. Use when asked to scout for AI news, find new tools, check what's trending in open-source AI, research recent model releases, or run a periodic AI intelligence sweep. Triggers on "what's new in AI", "scout", "AI news", "trending AI tools", "new models", "community updates".
---

# AI Scout

Scan open-source AI communities and surface discoveries relevant to the user's stack.

## Workflow

### 1. Load Context
- Read `references/stack-profile.md` — know what hardware/software is in play
- Read `references/sources.md` — get source list and search queries

### 2. Sweep Sources (rotate across runs)
Each run should cover a mix of source types. Don't hit all every time — rotate.

#### 2a. Web Search (Reddit, HN, GitHub — 3-5 queries per run)
Use `web_search` with queries from `references/sources.md`. Per sweep:
- Pick 3-5 queries (rotate across runs)
- Search each, collect top 3-5 results per query
- For promising hits, `web_fetch` the page for details

#### 2b. YouTube Channels (optional)

WormHole v0.1.0 does not ship a YouTube channel scout. If you want one, build `workspace/tools/youtube-scout.js` with a `--days` and `--max` interface and a `CHANNELS` array of channel IDs. The skill expects the tool to print latest videos with title, channel name, and URL. YouTube Data API quota is roughly 100 units per channel; the free tier is 10,000 units a day. See `references/sources.md` for the channel-list pattern.

#### 2c. Email Newsletters (optional)

WormHole v0.1.0 does not ship an IMAP newsletter reader. If you want one, build `workspace/tools/email-reader.js` with `--from`, `--hours`, and `--max` flags. Credentials go in the DPAPI vault, never inline. See `references/sources.md` for the suggested newsletter list.

### 3. Filter for Relevance
Apply `stack-profile.md` relevance filters. Discard anything that:
- Requires hardware the user doesn't have (>8GB VRAM, no CPU fallback)
- Is a rehash of a known tool already in the stack
- Is enterprise/cloud-only with no self-hosted path

### 4. Score & Rank
Rate each finding (1-5):
- **Impact**: How much would this improve the stack? (speed, cost, capability)
- **Effort**: How hard to integrate? (drop-in vs. build-from-source)
- **Maturity**: Production-ready vs. experimental?

Prioritize: High impact + Low effort + Mature > everything else.

### 5. Report Format
```
## 🔭 AI Scout Report — [DATE]

### 🔥 Top Finds
1. **[Tool/Model Name]** — one-line summary
   - Source: [link]
   - Why it matters: [relevance to stack]
   - Effort: [easy/medium/hard]
   - Verdict: [integrate now / watch / skip]

### 👀 Worth Watching
- [Tool] — [why] ([link])

### 📊 Community Pulse
- Hot topics this period: [themes]
- Trending repos: [names]

### ⏭️ Next Actions
- [ ] Specific integration steps for top finds
```

### 6. Memory & Auto-Learning
After each sweep:
- Log findings in `memory/YYYY-MM-DD.md`
- Update `references/stack-profile.md` if stack changes
- Track what's been reported to avoid repeats (check recent memory files)

**Auto-Learning (apply every run):**
- If a source repeatedly fails (DDG rate limit, 403, timeout): log as `[auto-learn]` in daily memory AND add to `references/sources.md` with a `⚠️ unreliable` note + fallback query
- If a source consistently returns low-relevance results: downgrade its priority in `references/sources.md`
- If a new source yields high-quality finds: promote it to High priority
- Track false positives: if the user ignores or dismisses a category of findings 3+ times, add a filter rule to step 3
- Never re-report the same tool/model within 7 days unless there's a significant update
- When a "WATCH" item from a prior sweep gets confirmed/debunked, update the original memory entry

**Episodic Memory (Reflexion pattern):**
Before each sweep, check the last 3 AI Scout entries in recent memory files. Ask:
- What sources worked well last time? Prioritize those.
- What sources failed or timed out? Skip or use fallback.
- What was already reported? Don't duplicate.
- Did any prior "INVESTIGATE" items get actioned? Follow up if not.

## Scheduling
This skill works well as a cron job. Recommended: 2x daily via isolated agent run on **Gemma4** (free local model).
Escalate to Sonnet only if Gemma4 consistently fails to produce useful output.

Example cron prompt:
> Run AI Scout: sweep open-source AI communities for new tools, models, and techniques relevant to my stack. Follow the ai-scout skill.
