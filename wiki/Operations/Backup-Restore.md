# Backup and Restore

`wormhole backup` writes an encrypted snapshot of the workspace (memory, sessions, modules, optionally the vault export). `wormhole restore <path>` round-trips it back. This page covers the default encrypted format, the `--unsafe-plain` opt-out, what is included by default versus what requires `--include-vault-export`, where backups live, how to verify a backup decrypts before you trust it, and the recovery path if your machine dies.

<!-- TODO: fill in during Phase 1 docs sprint -->
