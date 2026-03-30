# S02: Settings & Encryption HTTP Handlers

**Goal:** Create settings.rs and encryption.rs handler modules, register in routes.rs. Unlock broadcasts WS event.
**Demo:** After this: GET /settings, PUT /settings, GET /encryption/state, POST /encryption/unlock, POST /encryption/lock all respond correctly

## Tasks
- [x] **T01: Created settings.rs and encryption.rs HTTP handler modules for daemon API with GET/PUT /settings, GET /encryption/state, POST /encryption/unlock, POST /encryption/lock endpoints** — 1. Create settings.rs: pub fn router() with GET/PUT /settings routes
2. get_settings_handler: CoreUseCases::get_settings().execute(), return JSON with data+ts
3. update_settings_handler: parse Json<Settings> with JsonRejection, call update_settings().execute(). NO OS-level side effects (no autostart, no keyboard shortcuts)
4. Create encryption.rs: pub fn router() with GET /encryption/state, POST /encryption/unlock, POST /encryption/lock
5. get_encryption_state_handler: map EncryptionState + is_ready to wire format
6. unlock_handler: call UnlockEncryptionWithPassphrase, broadcast encryption.session-ready WS event on success. Map errors: NotInitialized→400, UnwrapFailed→401, others→500
7. lock_handler: call encryption_session.clear()
8. UnlockRequest must NOT derive Debug
  - Estimate: 45min
  - Files: src-tauri/crates/uc-daemon/src/api/settings.rs, src-tauri/crates/uc-daemon/src/api/encryption.rs
  - Verify: cd src-tauri && cargo check -p uc-daemon
- [x] **T02: Register settings and encryption routers in daemon L2+ HTTP router** — 1. In mod.rs, add pub mod encryption; pub mod settings;
2. In routes.rs router_l2_plus(), merge settings::router() and encryption::router()
3. Update routes.rs doc comment re: L3/L4 enforcement
4. Run full daemon test suite
  - Estimate: 10min
  - Files: src-tauri/crates/uc-daemon/src/api/routes.rs, src-tauri/crates/uc-daemon/src/api/mod.rs
  - Verify: cd src-tauri && cargo test -p uc-daemon -- --nocapture 2>&1 | tail -20
