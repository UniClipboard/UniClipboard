---
id: S01
parent: M002-zldd9y
milestone: M002-zldd9y
provides:
  - PermissionLevel L3Sensitive (value 3) and L4Dangerous (value 4) with from_u8 support
  - daemon_api_strings: ws_topic::ENCRYPTION, ws_event::ENCRYPTION_SESSION_READY, and 6 HTTP route constants
  - UnlockEncryptionWithPassphrase use case with 8 unit tests
  - CoreUseCases::unlock_encryption_with_passphrase() accessor
requires:
  []
affects:
  - S02
  - S03
key_files:
  - src-tauri/crates/uc-daemon/src/security/permission.rs
  - src-tauri/crates/uc-daemon/src/security/tests.rs
  - src-tauri/crates/uc-core/src/network/daemon_api_strings.rs
  - src-tauri/crates/uc-daemon/src/api/ws.rs
  - src-tauri/crates/uc-app/src/usecases/unlock_encryption_with_passphrase.rs
  - src-tauri/crates/uc-app/src/usecases/mod.rs
key_decisions:
  - Extended PermissionLevel from L1/L2-only (Phase 75) to L1–L4 (Phase 76) — both L3Sensitive (value 3) and L4Dangerous (value 4) now map from_u8 correctly
  - Error taxonomy mirrors AutoUnlockEncryptionSession but replaces KekLoadFailed with KekDeriveFailed — passphrase-supplied KDF vs keyring-loaded KEK is the only functional divergence
  - No Debug derive on anything holding Passphrase — UnlockWithPassphraseError uses thiserror defaults which omit sensitive fields automatically
patterns_established:
  - dyn Port pattern for use cases (following AutoUnlockEncryptionSession as template)
  - HTTP route + WS topic/event constants in daemon_api_strings as single source of truth
observability_surfaces:
  - none
drill_down_paths:
  - .gsd/milestones/M002-zldd9y/slices/S01/tasks/T01-SUMMARY.md
  - .gsd/milestones/M002-zldd9y/slices/S01/tasks/T02-SUMMARY.md
duration: ""
verification_result: passed
completed_at: 2026-03-30T00:56:16.483Z
blocker_discovered: false
---

# S01: Foundation: Permissions, Constants & Unlock Use Case

**Extended PermissionLevel to L1-L4, added Phase 76 daemon_api_strings constants, created UnlockEncryptionWithPassphrase use case with 8 unit tests**

## What Happened

Extended PermissionLevel enum from L1/L2-only (Phase 75) to L1–L4 (Phase 76), adding L3Sensitive (=3) and L4Dangerous (=4) variants with matching from_u8() support. Updated two stale tests in tests.rs that expected the old Phase-75 behavior. Added to daemon_api_strings: ws_topic::ENCRYPTION, ws_event::ENCRYPTION_SESSION_READY, and six HTTP route constants (SETTINGS, ENCRYPTION_STATE, ENCRYPTION_UNLOCK, ENCRYPTION_LOCK, STORAGE_STATS, STORAGE_CLEAR_CACHE). Added ENCRYPTION to is_supported_topic() in ws.rs. Implemented UnlockEncryptionWithPassphrase use case following the dyn Port pattern used by AutoUnlockEncryptionSession — flow: check state (must be Initialized) → resolve scope → load keyslot → derive KEK from passphrase+salt → unwrap MasterKey → set session. Error taxonomy mirrors AutoUnlock but replaces KekLoadFailed with KekDeriveFailed. 8 unit tests cover all error paths. CoreUseCases::unlock_encryption_with_passphrase() accessor wired to security ports from CoreRuntime.

## Verification

All 30 relevant tests pass: 10 permission tests, 7 daemon_api_strings tests, 5 is_supported_topic tests, 8 unlock_encryption_with_passphrase tests. All verification commands returned exit code 0.

Verification commands:
- cargo test -p uc-daemon permission -- 10 passed
- cargo test -p uc-core daemon_api_strings -- 7 passed
- cargo test -p uc-daemon is_supported_topic -- 5 passed
- cargo test -p uc-app unlock_encryption_with_passphrase -- 8 passed

## Requirements Advanced

None.

## Requirements Validated

None.

## New Requirements Surfaced

None.

## Requirements Invalidated or Re-scoped

None.

## Deviations

Updated two stale tests in src/security/tests.rs (permission_level_l3_returns_none → permission_level_l3_returns_l3_sensitive, permission_level_l4_returns_none → permission_level_l4_returns_l4_dangerous) that expected the old Phase-75 behavior. The task plan only listed permission.rs tests but the tests.rs file was part of the same module boundary and needed updating.

## Known Limitations

None.

## Follow-ups

None.

## Files Created/Modified

- `src-tauri/crates/uc-daemon/src/security/permission.rs` — Added L3Sensitive/L4Dangerous variants and from_u8 support for values 3 and 4
- `src-tauri/crates/uc-daemon/src/security/tests.rs` — Updated stale tests from Phase-75 to expect L3/L4 variants
- `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` — Added ws_topic::ENCRYPTION, ws_event::ENCRYPTION_SESSION_READY, and 6 HTTP route constants
- `src-tauri/crates/uc-daemon/src/api/ws.rs` — Added ENCRYPTION to is_supported_topic() function
- `src-tauri/crates/uc-app/src/usecases/unlock_encryption_with_passphrase.rs` — New use case with 8 unit tests covering all error paths
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — Module exports for unlock_encryption_with_passphrase
