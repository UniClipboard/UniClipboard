# M002-zldd9y: Daemon Settings, Encryption & Storage HTTP API — Context

**Gathered:** 2026-03-29
**Status:** Ready for planning
**Source:** PRD Express Path (docs/plans/frontend-direct-daemon-connection.md)
**Migrated from:** .planning/phases/76-daemon-settings-encryption-storage-api/76-CONTEXT.md

## Phase Boundary

Add daemon HTTP endpoints for settings (GET/PUT), encryption state management (get state, unlock, lock), and storage statistics (stats, clear cache). These complete the daemon API surface needed for frontend direct connection.

## Implementation Decisions

### Settings API

- GET `/settings` — return all settings (general, sync, security, privacy), permission L2
- PUT `/settings` — update settings, permission L3 (modifying settings is sensitive)
- Response type: SettingsResponse { general, sync, security, privacy }

### Encryption API

- GET `/encryption/state` — return current encryption state, permission L2
- POST `/encryption/unlock` — unlock encryption session with passphrase, permission L3
- POST `/encryption/lock` — lock encryption session, permission L3
- EncryptionStateResponse: { state: "uninitialized"|"locked"|"unlocked", has_keyslot, requires_passphrase }
- On successful unlock: broadcast WS event `encryption.session-ready`

### Storage API

- GET `/storage/stats` — return storage statistics, permission L2
- POST `/storage/clear-cache` — clear cache (requires confirmation), permission L4 (dangerous)
- StorageStatsResponse: { total_size_bytes, blob_count, database_size_bytes, cache_size_bytes, spool_size_bytes }
- ClearCache requires `{ confirmed: true }` in request body

### Permission Levels

- L2: settings read, encryption state read, storage stats
- L3: settings write, encryption unlock/lock
- L4: clear cache (dangerous, requires confirmation)

### Claude's Discretion

- How to map existing uc-app settings use cases to HTTP handlers
- Settings update request type granularity (full replacement vs partial patch)
- Whether encryption unlock needs additional rate limiting beyond global rate limiter
- Storage stats calculation approach (synchronous vs background)

## Canonical References

### Settings

- `src-tauri/crates/uc-app/src/usecases/settings/` — Settings use cases
- `src-tauri/crates/uc-core/src/settings/` — Settings domain models
- `src-tauri/crates/uc-tauri/src/commands/settings.rs` — Current settings command implementations

### Encryption

- `src-tauri/crates/uc-app/src/usecases/` — Encryption-related use cases
- `src-tauri/crates/uc-tauri/src/commands/encryption.rs` — Current encryption commands
- `src-tauri/crates/uc-infra/src/security/` — Encryption infrastructure

### Storage

- `src-tauri/crates/uc-tauri/src/commands/storage.rs` — Current storage commands
- `src-tauri/crates/uc-app/src/usecases/storage/` — Storage use cases

### Daemon API Patterns

- `src-tauri/crates/uc-daemon/src/api/` — Existing API patterns from Phase 74

## Specific Ideas

- Settings API should reuse existing settings model from uc-core, not create daemon-specific DTOs
- Encryption unlock flow must coordinate with daemon's EncryptionSessionState
- Clear cache L4 operation should emit a confirmation-required error if `confirmed` field is missing/false
- WS event for encryption state changes enables frontend to react without polling

## Deferred Ideas

- Per-field settings validation (complex business rules)
- Settings change history/audit trail
- Encryption key rotation API
- Storage quota enforcement API