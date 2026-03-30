---
id: T02
parent: S01
milestone: M002-zldd9y
provides: []
requires: []
affects: []
key_files: ["src-tauri/crates/uc-app/src/usecases/unlock_encryption_with_passphrase.rs", "src-tauri/crates/uc-app/src/usecases/mod.rs"]
key_decisions: ["Error taxonomy mirrors AutoUnlockEncryptionSession but replaces KekLoadFailed with KekDeriveFailed — passphrase-supplied KDF vs keyring-loaded KEK is the only functional divergence", "No Debug derive on anything holding Passphrase — UnlockWithPassphraseError uses thiserror defaults which omit sensitive fields automatically"]
patterns_established: []
drill_down_paths: []
observability_surfaces: []
duration: ""
verification_result: "Ran `cargo test -p uc-app unlock_encryption -- --nocapture` — 16 tests matched (8 new unlock_encryption_with_passphrase tests + 8 pre-existing auto_unlock_encryption_session tests). All 16 pass with exit code 0."
completed_at: 2026-03-30T00:53:54.632Z
blocker_discovered: false
---

# T02: Created UnlockEncryptionWithPassphrase use case with 8 unit tests and CoreUseCases accessor

> Created UnlockEncryptionWithPassphrase use case with 8 unit tests and CoreUseCases accessor

## What Happened
---
id: T02
parent: S01
milestone: M002-zldd9y
key_files:
  - src-tauri/crates/uc-app/src/usecases/unlock_encryption_with_passphrase.rs
  - src-tauri/crates/uc-app/src/usecases/mod.rs
key_decisions:
  - Error taxonomy mirrors AutoUnlockEncryptionSession but replaces KekLoadFailed with KekDeriveFailed — passphrase-supplied KDF vs keyring-loaded KEK is the only functional divergence
  - No Debug derive on anything holding Passphrase — UnlockWithPassphraseError uses thiserror defaults which omit sensitive fields automatically
duration: ""
verification_result: passed
completed_at: 2026-03-30T00:53:54.633Z
blocker_discovered: false
---

# T02: Created UnlockEncryptionWithPassphrase use case with 8 unit tests and CoreUseCases accessor

**Created UnlockEncryptionWithPassphrase use case with 8 unit tests and CoreUseCases accessor**

## What Happened

Implemented UnlockEncryptionWithPassphrase use case following the dyn Port pattern used by AutoUnlockEncryptionSession. The use case: checks encryption state (must be Initialized), resolves key scope, loads keyslot, derives KEK from the user-provided passphrase + stored salt + KDF params, unwraps the MasterKey using the derived KEK, and sets the MasterKey in the EncryptionSessionPort. Error taxonomy mirrors AutoUnlock but replaces KekLoadFailed with KekDeriveFailed (passphrase-derived vs keyring-loaded). Eight unit tests cover all error paths (NotInitialized, KeySlotLoadFailed, MissingWrappedMasterKey, ScopeFailed, SessionSetFailed, StateCheckFailed, wrong passphrase/KekDeriveFailed, wrong passphrase/UnwrapFailed). CoreUseCases::unlock_encryption_with_passphrase() accessor wired to security ports from CoreRuntime.

## Verification

Ran `cargo test -p uc-app unlock_encryption -- --nocapture` — 16 tests matched (8 new unlock_encryption_with_passphrase tests + 8 pre-existing auto_unlock_encryption_session tests). All 16 pass with exit code 0.

## Verification Evidence

| # | Command | Exit Code | Verdict | Duration |
|---|---------|-----------|---------|----------|
| 1 | `cargo test -p uc-app unlock_encryption -- --nocapture` | 0 | ✅ pass | 27800ms |


## Deviations

None

## Known Issues

None

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/unlock_encryption_with_passphrase.rs`
- `src-tauri/crates/uc-app/src/usecases/mod.rs`


## Deviations
None

## Known Issues
None
