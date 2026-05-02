# Cron System

The cron system is `cron.toml` plus `cron.d/*.toml` plus `cron.rs`. Each cron entry has an id, a schedule expression, a kind (a function or skill to invoke), and optional payload data. The runner fires jobs on schedule, captures runs to `cron-runs.jsonl`, and tracks state in `cron-state.json`. This page covers the schedule format, the `kind` taxonomy, the per-tool capability allowlist that gates which tools a cron job can call, the missed-job catch-up policy, and how to debug a job that is not firing.

<!-- TODO: fill in during Phase 1 docs sprint -->
