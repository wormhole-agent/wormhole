# Getting Started with WormHole

This is the zero-to-first-conversation walkthrough. By the end you will have WormHole running on your machine, a dashboard open in your browser, and the agent answering a Telegram message or a CLI prompt. Plan on 15 to 20 minutes if everything cooperates, longer if you need to install Rust first.

The walkthrough assumes Windows 10 or 11. macOS and Linux support is on the roadmap (via Tauri). For now, everything below is Windows-flavored.

---

## What you will end up with

- A `wormhole.exe` binary in `target/release/` that you can run from any terminal.
- A `~/wormhole/` directory holding your config, your encrypted vault, and your `.token`.
- A `~/wormhole/workspace/` directory holding memory, skills, modules, and tools.
- A dashboard at `http://127.0.0.1:18790/` showing what the agent is doing.
- Optionally, a Telegram bot you can DM to talk to the agent from your phone.

---

## Step 1: Install Rust

If you do not have a Rust toolchain, install one. The official installer (rustup) gets you the compiler, the package manager, and the standard library.

Open PowerShell and run:

```
winget install Rustlang.Rustup
```

Or download `rustup-init.exe` from `https://rustup.rs/` and run it. Either path takes about three minutes.

Verify:

```
rustc --version
cargo --version
```

You want both to print something. If they error with "command not found," restart your terminal so the PATH update takes effect.

WormHole builds against stable Rust, edition 2021. Any stable release from the last two years works.

---

## Step 2 (optional): Install Node.js

WormHole's binary does not need Node. The workspace **tools** (Node scripts under `workspace/tools/`) need Node 20 LTS if you plan to use them. If you only want to chat with the agent over Telegram or the dashboard, you can skip this step and come back to it later.

```
winget install OpenJS.NodeJS.LTS
```

Verify:

```
node --version
npm --version
```

---

## Step 3 (optional): Install Ollama

If you want a local LLM fallback (so the agent works without an internet connection or external API keys), install Ollama.

1. Download from `https://ollama.com/download`.
2. Run the installer.
3. Pull a model: `ollama pull llama3.2:3b` (or any model you like).
4. Verify Ollama is running: `curl http://127.0.0.1:11434/api/tags` should return a JSON list of installed models.

WormHole probes this endpoint at startup and offers to wire any detected model as the local fallback.

---

## Step 4: Clone and build WormHole

```
git clone https://github.com/wormhole-agent/wormhole
cd wormhole
cargo build --release
```

The release build takes a few minutes the first time (cargo has to compile every dependency from scratch). Subsequent builds are fast because cargo caches everything.

When it finishes, you have a binary at `target\release\wormhole.exe`. You can run it directly, or copy it somewhere on your PATH.

If you want a smaller binary without the embedded dashboard:

```
cargo build --release --no-default-features
```

---

## Step 5: Initialize the workspace

```
.\target\release\wormhole.exe init
```

This creates:

- `%USERPROFILE%\wormhole\`: your home directory for the binary's state.
- `%USERPROFILE%\wormhole\config.toml`: generated from the template, ready to edit.
- `%USERPROFILE%\wormhole\.token`: a 256-bit random token that gates the dashboard API.
- `%USERPROFILE%\wormhole\workspace\`: a fresh, generic workspace with `SOUL.md`, `AGENTS.md`, `MEMORY.md` (template), the `example-module/`, and an empty `tools/` and `skills/` ready for you to fill.

The init command is idempotent. Re-running it does not clobber anything you have edited.

---

## Step 6: Add your API keys

You need at least one LLM provider. Pick one to start; you can add more later.

### Option A: paste keys via the vault command

```
.\target\release\wormhole.exe vault edit
```

This opens an editable plaintext buffer (in memory only, never written to disk in plaintext), prompts you to paste keys, then encrypts the result to `secrets.md.enc` using Windows DPAPI bound to your user account.

Recommended keys:
- `ANTHROPIC_API_KEY`: primary provider for tool-using conversations. Sign up at `console.anthropic.com`.
- `OPENAI_API_KEY`: general-purpose fallback. Sign up at `platform.openai.com`.
- `DEEPSEEK_API_KEY`: used for the V4 critique gate. Sign up at `platform.deepseek.com`.
- `TELEGRAM_BOT_TOKEN`: only if you want the Telegram channel. Get one from @BotFather on Telegram.

### Option B: environment variables

You can also set keys as env vars and skip the vault entirely:

```powershell
$env:ANTHROPIC_API_KEY = "sk-ant-..."
$env:OPENAI_API_KEY = "sk-..."
```

Env vars override anything in the vault. Useful for testing or for shared machines where you do not want keys to persist.

---

## Step 7 (optional): Back up your vault

DPAPI ties the vault to your Windows account. If your account is corrupted, deleted, or you switch machines, the vault becomes unrecoverable. The fix is a portable export:

```
.\target\release\wormhole.exe vault export
```

This prompts for a passphrase and writes `~/wormhole/workspace/secrets.export.age`. Keep that file somewhere safe (a USB drive, a password manager, a different machine). On a new machine, `wormhole vault import-export <path>` round-trips it back.

This step is optional for v0.1.0 (build-from-source users tend to know what they are doing) but skipping it means a Windows-account loss is also a key loss.

---

## Step 8: Start the daemon

```
.\target\release\wormhole.exe serve
```

You will see log lines about providers loading, cron jobs registering, the UI binding to `127.0.0.1:18790`, and the Telegram bot starting (if you configured it).

Leave this running.

---

## Step 9: First conversation

You have three options. Pick whichever fits.

### CLI

In a second terminal:

```
.\target\release\wormhole.exe ask "What can you do?"
```

The agent answers on stdout.

### Dashboard

Open `http://127.0.0.1:18790/` in your browser. You will be prompted for the token. Find it at `~/wormhole/.token` (cat that file, copy the value). The dashboard renders widgets driven by your cron jobs (which start out mostly empty until cron has had time to run).

### Telegram

If you wired Telegram, find your bot in the Telegram app (search by the bot's name) and send `/ping`. The agent replies. From then on, any DM is a conversation.

**A note on what to expect.** The agent is set up with a default operating principle: **she should look for, find, and build her own tools before asking you for help.** When you give her a task that needs something she does not have (an API client, a CLI wrapper, a new skill), her first move is to check what is already installed. Her second move is to look for an open-source thing she can install. Only when those fail does she come back to you with a "can I install this?" question. This is the rule encoded in `workspace/skills/find_or_build_capability.md`. Read it when you want to understand how she will behave.

---

## What to do next

- **Read [`workspace/AGENTS.md`](./workspace/AGENTS.md).** It is the agent's operating rulebook. It also tells you what tone to use when you ask her to do things.
- **Rename your worm.** The default personality is named "Larry." Edit `workspace/SOUL.md` to give her your own name and tone. The personality file is meant to be customized.
- **Try writing a skill.** Copy `workspace/skills/humanizer.md`, edit the front-matter and the body, save it, and ask the agent to use it. The wiki page **Getting Started -> Your First Skill** has a full walkthrough.
- **Try writing a cron job.** Add a TOML entry under `cron.d/`, name a kind, set a schedule. The wiki page **Getting Started -> Your First Cron** walks through it end to end.
- **Wander the wiki.** The Architecture section is the right read if you want to understand how the pieces fit. The Reference section is what you keep open in another tab while you write a config.

---

## When something does not work

- `wormhole doctor` is the first stop. It checks the local install: binary present, vault decrypts, config parses, ports free, scheduled task running, logs fresh. By default it never makes a network call. Add `--online` if you want it to ping providers.
- `~/wormhole/logs/wormhole.log` is the rolling log. Check the most recent file first.
- The wiki page **Operations -> Troubleshooting** has the canned recovery procedures.
- Real bugs go to GitHub Issues. Security issues go through `SECURITY.md`'s private channel; do not file public issues for security.

---

## A note on running unattended

The walkthrough above runs WormHole in a foreground terminal. For a "run all the time" setup you want a Windows scheduled task that starts `wormhole.exe serve` at login (and a watchdog that restarts it if it dies). Phase 1 ships an XML template (`wormhole-task.xml` in the repo) you can register with Task Scheduler. Phase 2 will register it for you as part of the MSI install.

---

End of walkthrough. Open issues if anything was unclear. The whole point of the docs is that they get sharper every time someone gets stuck.
