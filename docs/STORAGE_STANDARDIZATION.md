# Storage Standardization

Last updated: 2026-02-13

## Canonical Storage Contract

Shared runtime data for Tandem clients now uses one canonical root:

- Windows: `%APPDATA%/tandem`
- macOS/Linux: `${XDG_DATA_HOME:-~/.local/share}/tandem`

Legacy source root (read/migrate only):

- `%APPDATA%/ai.frumu.tandem` (and platform-equivalent via data-dir)

## Shared Artifacts (Canonical)

- `vault.key`
- `tandem.keystore`
- `memory.sqlite` (+ `-shm`/`-wal`)
- `config.json`
- `data/` (engine state)
- `storage/` / `state/` (legacy-compatible engine/session dirs)
- `sidecar_release_cache.json`
- `logs/`
- `storage_version.json`
- `migration_report.json`

## Migration Policy

- Mode: **copy + keep legacy** (non-destructive).
- Migration now runs through a startup wizard (blocking UI overlay) and can be re-run from Settings.
- Startup auto-run triggers after vault unlock when legacy sources are detected.
- Source discovery order includes:
- `%APPDATA%/ai.frumu.tandem`
- `%APPDATA%/opencode`
- `%USERPROFILE%/.local/share/opencode`
- Copy uses guarded behavior:
- skip when destination already exists with same length and newer/equal mtime
- copy otherwise
- No auto-delete/rename of legacy root.
- Migration writes:
- `storage_version.json` (layout marker)
- `migration_report.json` (reason, copied/skipped/errors, timestamp)
- Startup progress events:
- `storage-migration-progress`
- `storage-migration-complete`

## Cross-Client Wiring

- `tandem-core::resolve_shared_paths()` is the source of truth for all clients.
- `tandem-core::migrate_legacy_storage_if_needed()` performs idempotent startup migration.
- `tandem-engine` defaults `state_dir` to canonical `data/` unless overridden by:

1. `--state-dir`
2. `TANDEM_STATE_DIR`

- Tauri sidecar launch always passes explicit `--state-dir <canonical data dir>`.
- Tauri keystore/vault/memory/tool-history paths resolve from canonical root.
- TUI boot now uses the same shared path resolver + migrator.

## Diagnostics

Tauri commands:

- `get_storage_status()`
- `get_storage_migration_status()`
- `run_storage_migration(options)`

Returns canonical/legacy roots and marker/report presence so UI and future TUI adapters can verify migration state deterministically.
