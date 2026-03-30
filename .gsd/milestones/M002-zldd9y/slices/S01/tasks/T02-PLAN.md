---
estimated_steps: 7
estimated_files: 2
skills_used: []
---

# T02: Create UnlockEncryptionWithPassphrase use case with tests and CoreUseCases accessor

1. Create unlock_encryption_with_passphrase.rs in uc-app/src/usecases/
2. Define UnlockWithPassphraseError enum (NotInitialized, ScopeFailed, KeySlotLoadFailed, MissingWrappedMasterKey, KekDeriveFailed, UnwrapFailed, SessionSetFailed)
3. Implement execute(passphrase: Passphrase) flow: check state → get scope → load keyslot → derive KEK → unwrap master key → set session
4. Do NOT derive Debug on anything holding Passphrase
5. Add unit tests: uninitialized error, success path, wrong passphrase error
6. Add pub mod and re-export in mod.rs
7. Add CoreUseCases::unlock_encryption_with_passphrase() accessor

## Inputs

- `src-tauri/crates/uc-app/src/usecases/auto_unlock_encryption_session.rs`
- `src-tauri/crates/uc-app/src/usecases/mod.rs`
- `src-tauri/crates/uc-core/src/ports/security/encryption.rs`
- `src-tauri/crates/uc-core/src/security/model.rs`

## Expected Output

- `unlock_encryption_with_passphrase.rs with use case and tests`
- `CoreUseCases accessor wired`

## Verification

cd src-tauri && cargo test -p uc-app unlock_encryption_with_passphrase -- --nocapture
