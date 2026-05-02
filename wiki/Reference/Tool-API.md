# Tool API Reference

The full tool surface: `shell`, `read_file`, `write_file`, `http_get`, `delegate`, `load_skill`, plus the conventions for adding a new tool. Each tool gets its argument schema, its return shape, its error modes, and the capability flag that gates its use. This page also covers the path-sandboxing logic in `tools.rs::is_within`, the per-tool capability allowlist that cron jobs must declare, and the audit log every tool call writes.

<!-- TODO: fill in during Phase 1 docs sprint -->
