# Cron Schema Reference

The TOML schema for cron entries: every field, every allowed value, every default. This is the page you have open in another tab while you write a cron job. Sections: top-level fields (`id`, `kind`, `schedule`, `enabled`, `catch_up`), kind-specific payloads (`shell`, `tool`, `skill`, `dream`), the per-tool capability allowlist that gates which tools the job can call, schedule expression syntax, and validation errors and what they mean.

<!-- TODO: fill in during Phase 1 docs sprint -->
