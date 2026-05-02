# AI Scout Sources

## Reddit (via web_search `site:reddit.com`)
| Subreddit | Focus | Priority |
|-----------|-------|----------|
| r/OpenClaw | OpenClaw updates, skills, plugins, config tips | High |
| r/LocalLLaMA | Local model releases, quantization, Ollama, inference speed | ⚠️ unreliable via Brave — 4 zero-result sweeps (2026-04-17/19/20/21). DOWNGRADED to Low via Brave. Default to HN Algolia + GitHub Trending + ollama.com/library for local model discovery. Only retry Brave r/LocalLLaMA monthly to re-test indexing |
| r/MachineLearning | Papers, breakthroughs, new architectures | Medium |
| r/selfhosted | Self-hosted AI tools, infra, privacy-first setups | Medium |
| r/Ollama | Ollama-specific tips, model configs, performance | High |
| r/artificial | General AI news, policy, industry trends | Medium |
| r/singularity | Cutting-edge breakthroughs, AGI chatter, paradigm shifts | Medium |
| r/ChatGPT | Mainstream AI tool updates, prompt techniques | Low |
| r/ClaudeAI | Anthropic updates, Claude tips, API changes | Medium |
| r/OpenAI | OpenAI releases, GPT updates, API changes | Medium |
| r/ArtificialIntelligence | Broad AI discussion, newcomer-friendly finds | Low |
| r/StableDiffusion | Image gen models (lower priority unless multimodal) | Low |

## Hacker News
- Search: `site:news.ycombinator.com` + AI/ML terms
- Or: `https://hn.algolia.com/api/v1/search?query=<term>&tags=story&numericFilters=created_at_i>UNIX_TS`
- Focus: front-page AI stories, new tool launches, benchmark results

## GitHub Trending
- `https://github.com/trending?since=weekly&spoken_language_code=en`
- Filter for: ML/AI repos, Ollama integrations, agent frameworks, quantization tools
- Also check: `https://github.com/topics/llm` and `https://github.com/topics/ollama`

## YouTube Channels
- Fetch latest videos via: `site:youtube.com/@HANDLE` in web_search
- For transcripts/summaries: use `web_fetch` on video pages, or search `"Creator Name" AI` for coverage
- Rotate through channels across sweeps — don't check all every run

| Channel | Handle | Focus | Priority |
|---------|--------|-------|----------|
| Nate B. Jones | @NateBJones | AI strategy, frameworks, workflows for builders & execs | High |
| Matthew Berman | @matthew_berman | Open-source AI, local models, new releases, benchmarks | High |
| Fireship | @Fireship | Dev-focused AI news, fast "100 seconds" explainers, shipping code | High |
| AI Explained | @aiexplained-official | In-depth balanced analysis of AI developments, research, ethics | High |
| Matt Wolfe | @mreflow | AI tool discovery, weekly news roundups (runs FutureTools.io) | Medium |
| The AI Advantage | @aiadvantage | Practical AI for business, productivity workflows | Medium |
| All About AI | @AllAboutAI | Local LLMs, Ollama, self-hosted AI tutorials, agent builds | High |
| Corbin Brown | @Corbin_Brown | AI coding tutorials, Claude/Cursor workflows, building with AI | Medium |
| Machine Learning Street Talk | @MachineLearningStreetTalk | Deep-dive expert interviews, research discussion | Low |
| Sabrina Ramonov | @sabrina_ramonov | AI agents, automation, prompts for solopreneurs | Medium |

## ArXiv (lightweight)
- Only when a paper is buzzing on Reddit/HN
- Use `https://arxiv.org/abs/<id>` to fetch abstract
- Don't deep-dive papers unless explicitly relevant

## Email Newsletters (optional, requires user-supplied IMAP tool)

> WormHole v0.1.0 does NOT ship a gmail/IMAP reader. If you want to pipe newsletters into the scout, write a tiny tool at `workspace/tools/email-reader.js` that pulls from your IMAP account and prints message bodies. The skill expects the tool to accept `--from` and `--hours` flags. Credentials go in the DPAPI vault, never inline.

| Newsletter | Sender Filter | Focus | Priority |
|-----------|--------------|-------|----------|
| TLDR AI | `--from "TLDR"` | Daily AI/ML digest, tool launches, research | High |
| TLDR InfoSec | `--from "TLDR"` | Security + AI intersection | Medium |
| The Hustle | `--from "Hustle"` | Business/tech news, startup trends | Medium |
| Ideabrowser | `--from "Ideabrowser"` | Startup ideas, market gaps | Medium |
| TLDR Founders | `--from "TLDR"` | Founder tactics, growth | Low |
| TLDR Dev | `--from "TLDR"` | Developer tools, frameworks | Medium |

## YouTube Channels (optional, requires user-supplied scout tool)

> WormHole v0.1.0 does NOT ship a YouTube-channel scout tool (it requires a YouTube Data API key, which is per-user). If you want this, write `workspace/tools/youtube-scout.js` with a `CHANNELS` array of channel IDs and a `--days` / `--max` interface. Quota is roughly 100 units per channel; YouTube's free tier is 10,000 units a day.

Once shipped, the skill calls it during the sweep and the scout filters new uploads against your relevance filters.

## Search Queries (rotate through)
```
"ollama" new model site:reddit.com
"local llm" breakthrough site:reddit.com
"openclaw" site:reddit.com
"quantization" llm new site:reddit.com
"agent framework" open source 2026
"mixture of experts" local site:reddit.com
ollama tool calling site:reddit.com
"vram" optimization llm site:reddit.com
self-hosted ai new tool site:reddit.com
open source ai release this week
"claude" new feature site:reddit.com/r/ClaudeAI
"openai" release site:reddit.com/r/OpenAI
"AGI" breakthrough site:reddit.com/r/singularity
site:reddit.com/r/artificial AI news this week
```
