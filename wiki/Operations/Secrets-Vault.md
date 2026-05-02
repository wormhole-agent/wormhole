# Secrets Vault

API keys live in `secrets.md.enc`, encrypted with Windows DPAPI bound to your user SID. Plaintext only exists during explicit edit windows. This page covers the vault commands (`vault edit`, `vault export`, `vault import-export`), the DPAPI threat model and why the vault is per-Windows-user, the age-encrypted portable export for cross-machine recovery, the inter-process leak risk on the accessor and how it is mitigated, and the recovery path when a Windows account is corrupted.

<!-- TODO: fill in during Phase 1 docs sprint -->
