# M002-zldd9y: Daemon Settings, Encryption & Storage HTTP API — Research

**Researched:** 2026-03-30
**Domain:** Daemon HTTP API extension — settings, encryption state, storage stats
**Confidence:** HIGH
**Migrated from:** .planning/phases/76-daemon-settings-encryption-storage-api/76-RESEARCH.md

---

## Summary

Phase 76 adds three new API modules to the daemon HTTP server: settings read/write, encryption state management, and storage statistics/cache control. This phase is the first to require L3/L4 permission enforcement, which Phase 75 explicitly deferred. The permission system already has `PermissionLevel` enum defined (L1, L2) but L3 and L4 are not yet implemented — this phase must add them.

The implementation pattern is clear from Phase 74 (clipboard API): create a new `src/api/` submodule (e.g., `settings.rs`, `encryption.rs`, `storage.rs`) that follows the same handler structure as `clipboard.rs`. Use `CoreUseCases::new(runtime.as_ref())` pattern to access use cases. Register routes in `router_l2_plus()` in `routes.rs`.

The settings and storage use cases already exist in `uc-app` and are used by Tauri commands. Encryption unlock via passphrase requires a new use case — `AutoUnlockEncryptionSession` uses keyring (no passphrase), `InitializeEncryption` sets up a new passphrase. A new `UnlockEncryptionSessionWithPassphrase` use case must be built following the same pattern (load keyslot, derive KEK from passphrase, unwrap master key, set in session).

**Primary recommendation:** Follow the `clipboard.rs` module pattern exactly. Add L3/L4 to `permission.rs`, implement per-route permission middleware, and create three new handler modules. The encryption unlock use case is the only net-new domain logic.

## Standard Stack

No new dependencies. All required crates are already in the workspace: axum, serde/serde_json, uc-app (CoreUseCases), uc-core, tokio, tracing.

## Architecture Patterns

### Route Module Pattern (from Phase 74)

Each new API domain gets its own file: `src/api/settings.rs`, `src/api/encryption.rs`, `src/api/storage.rs`. Each exposes a `pub fn router() -> Router<DaemonApiState>` that is `.merge()`d into `router_l2_plus()` in `routes.rs`.

### Handler Pattern (from clipboard.rs)

Extract `State(state)`, get runtime, create `CoreUseCases::new(runtime.as_ref())`, call use case, return JSON response.

### L3/L4 Permission Enforcement

Phase 75 defined `PermissionLevel` enum with only `L1Public` and `L2Authenticated`. Phase 76 extends with `L3Sensitive` and `L4Dangerous`. L3 is a semantic label (same enforcement as L2 for now). L4 adds body confirmation check (`confirmed: true`).

### Encryption State Mapping

| EncryptionState | is_ready() | Wire value      | has_keyslot | requires_passphrase |
| --------------- | ---------- | --------------- | ----------- | ------------------- |
| Uninitialized   | false      | "uninitialized" | false       | false               |
| Initialized     | false      | "locked"        | true        | true                |
| Initialized     | true       | "unlocked"      | true        | false               |

### Unlock With Passphrase — New Use Case

`UnlockEncryptionWithPassphrase` steps:
1. Load EncryptionState — if Uninitialized, return error
2. Get KeyScope from KeyScopePort
3. Load KeySlot from KeyMaterialPort::load_keyslot()
4. Derive KEK from passphrase using EncryptionPort::derive_kek()
5. Unwrap master key using EncryptionPort::unwrap_master_key()
6. Set master key via EncryptionSessionPort::set_master_key()

### StorageStatsResponse Field Mapping

- `total_size_bytes` ← `total_bytes` + spool_bytes
- `database_size_bytes` ← `database_bytes`
- `cache_size_bytes` ← `cache_bytes`
- `spool_size_bytes` ← computed from `spool_dir` (not in GetStorageStats)
- `blob_count` ← from clipboard stats total_count (proxy)

## Common Pitfalls

1. **spool_size_bytes Not in GetStorageStats** — compute separately from `runtime.storage_paths().spool_dir`
2. **blob_count Not in GetStorageStats** — use clipboard stats `total_count` as proxy
3. **L3/L4 Not Yet Enforced** — must add variants and enforcement before registering routes
4. **Settings Update Side Effects** — daemon handler calls ONLY `update_settings().execute()`, no autostart/keyboard shortcuts
5. **Encryption Unlock Passphrase in Logs** — UnlockRequest must NOT derive Debug
6. **WS Event Type String** — add constant to `daemon_api_strings`, don't hardcode

## Open Questions

1. **blob_count source** — Recommendation: Use clipboard stats `total_count` as approximation
2. **spool_size_bytes source** — Recommendation: Compute from spool_dir in handler or extend GetStorageStats
3. **L3 enforcement semantics** — Recommendation: L3 = authenticated only (same as L2), semantic label for sensitive mutations
4. **encryption WS topic naming** — Recommendation: `ws_topic::ENCRYPTION = "encryption"`

## Sources

- Direct code inspection: `src-tauri/crates/uc-daemon/src/api/` — existing route pattern
- Direct code inspection: `src-tauri/crates/uc-daemon/src/security/permission.rs` — L1/L2 defined
- Direct code inspection: `src-tauri/crates/uc-app/src/usecases/` — all referenced use cases
- Direct code inspection: `src-tauri/crates/uc-core/src/settings/model.rs` — Settings struct
- Direct code inspection: `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` — constant patterns
- CONTEXT.md analysis — all implementation decisions locked by user

**Confidence:** HIGH | **Valid until:** 2026-04-30