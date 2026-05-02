# WormHole Binary

The `wormhole.exe` is one Rust binary, about 6,700 lines of code, built on tokio and axum. It hosts the brain (provider calls, tool iteration), the cron runner, the Telegram bot, the embedded HTTP server with dashboard, and the dreaming pass. This page maps every source module to the part of the system it owns, names the public commands (`serve`, `test`, `cron-list`, `cron-run`, `ask`, `dream`), and explains the boot sequence one phase at a time.

<!-- TODO: fill in during Phase 1 docs sprint -->
