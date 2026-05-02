# First Run

What happens the first time you start WormHole: the binary creates `~/wormhole/`, generates a 256-bit `.token`, prompts you for API keys, writes them into the DPAPI-encrypted vault, asks which channels you want (Telegram, CLI, dashboard), tries to detect a local Ollama, and hands you a working `/api/status` endpoint at `127.0.0.1:18790`. This page covers each step, what is optional, what to do if something fails, and where to find the install breadcrumbs in the log.

<!-- TODO: fill in during Phase 1 docs sprint -->
