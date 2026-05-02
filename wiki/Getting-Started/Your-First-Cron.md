# Your First Cron

Cron jobs are the agent's heartbeat. They run on a schedule, do something small, write the result somewhere the dashboard can render. This page walks through adding a simple cron job: pick a schedule, write the TOML entry under `cron.d/`, run it manually with `wormhole cron-run <id>`, and watch the result show up in the dashboard. It also covers the missed-cron catch-up policy, the per-tool capability allowlist, and how to stop a cron job that is misbehaving.

<!-- TODO: fill in during Phase 1 docs sprint -->
