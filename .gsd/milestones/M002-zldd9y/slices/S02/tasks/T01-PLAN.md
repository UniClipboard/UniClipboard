---
estimated_steps: 8
estimated_files: 2
skills_used: []
---

# T01: Create settings.rs and encryption.rs HTTP handler modules

1. Create settings.rs: pub fn router() with GET/PUT /settings routes
2. get_settings_handler: CoreUseCases::get_settings().execute(), return JSON with data+ts
3. update_settings_handler: parse Json<Settings> with JsonRejection, call update_settings().execute(). NO OS-level side effects (no autostart, no keyboard shortcuts)
4. Create encryption.rs: pub fn router() with GET /encryption/state, POST /encryption/unlock, POST /encryption/lock
5. get_encryption_state_handler: map EncryptionState + is_ready to wire format
6. unlock_handler: call UnlockEncryptionWithPassphrase, broadcast encryption.session-ready WS event on success. Map errors: NotInitialized→400, UnwrapFailed→401, others→500
7. lock_handler: call encryption_session.clear()
8. UnlockRequest must NOT derive Debug

## Inputs

- `src-tauri/crates/uc-daemon/src/api/clipboard.rs`
- `src-tauri/crates/uc-daemon/src/api/routes.rs`
- `src-tauri/crates/uc-daemon/src/api/server.rs`
- `src-tauri/crates/uc-daemon/src/api/types.rs`
- `src-tauri/crates/uc-tauri/src/commands/settings.rs`
- `src-tauri/crates/uc-tauri/src/commands/encryption.rs`

## Expected Output

- `src-tauri/crates/uc-daemon/src/api/settings.rs`
- `src-tauri/crates/uc-daemon/src/api/encryption.rs`

## Verification

cd src-tauri && cargo check -p uc-daemon
