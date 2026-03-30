# S01: Foundation: Permissions, Constants & Unlock Use Case — UAT

**Milestone:** M002-zldd9y
**Written:** 2026-03-30T00:56:16.483Z

# S01 UAT: Artifact-Driven Verification

## Preconditions
- Rust toolchain installed
- Project builds: `cd src-tauri && cargo build` completes without errors

## Smoke Test
`cargo test -p uc-app unlock_encryption_with_passphrase -- --nocapture` — expects 8 tests pass

## Test Cases
1. PermissionLevel L3/L4 from_u8 mapping (3 tests)
2. PermissionLevel roundtrip via tests.rs (2 tests)
3. daemon_api_strings HTTP route constants (1 test)
4. daemon_api_strings WS topic/event constants (2 tests)
5. is_supported_topic includes ENCRYPTION (1 test)
6. UnlockEncryptionWithPassphrase: NotInitialized error path
7. UnlockEncryptionWithPassphrase: happy path
8. UnlockEncryptionWithPassphrase: wrong passphrase error
9. UnlockEncryptionWithPassphrase: MissingWrappedMasterKey
10. UnlockEncryptionWithPassphrase: KeySlotLoadFailed, SessionSetFailed, ScopeFailed, StateCheckFailed (4 tests)

## Edge Cases Covered
- Missing keyslot load failure
- Session set failure (terminal error)
- Scope resolution failure (before keyslot access)
- State check failure propagation

## Not Proven
- Live HTTP handler integration (S02)
- Live storage stats integration (S03)
- End-to-end unlock with real on-disk keyslot
