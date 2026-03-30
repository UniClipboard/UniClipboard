---
sliceId: S02
uatType: live-runtime
verdict: PASS
date: 2026-03-30T01:45:00.000Z
---

# UAT Result — S02

## Checks

| Check | Mode | Result | Notes |
|-------|------|--------|-------|
| TC1: GET /settings returns 200 with {data, ts} | runtime | PASS | HTTP 200, body has full settings object with schema_version, general, sync, security, etc. |
| TC2: PUT /settings partial merge | runtime | PASS | PUT returns 200, GET confirms only targeted fields changed (device_name), others unchanged (theme, auto_start, etc.) |
| TC3: POST /encryption/unlock wrong passphrase → 401 | runtime | PASS | HTTP 401 with code "wrong_passphrase" |
| TC4: POST /encryption/unlock correct passphrase → 200 + WS event | runtime | PASS | HTTP 200 with success:true; WS receives encryption.session_ready event with ts in payload |
| TC5: POST /encryption/lock → sessionReady:false | runtime | PASS | HTTP 200; GET /encryption/state confirms sessionReady:false |
| Edge: Malformed JSON in PUT /settings → 400 | runtime | PASS | HTTP 400 with code "bad_request" |
| Edge: Empty passphrase in unlock → 401 | runtime | PASS | HTTP 401 with code "wrong_passphrase" |
| Edge: Unlock on uninitialized encryption → 400 | runtime | PASS | HTTP 400 with code "not_initialized" |

## Overall Verdict

PASS — All 8 checks passed. The Settings and Encryption HTTP handlers work correctly end-to-end including WS broadcast on unlock.

## Notes

### Bug Discovered and Fixed During UAT

During TC4 WS event testing, discovered a bug in `src-tauri/crates/uc-daemon/src/api/ws.rs`:
- `build_snapshot_event()` had no match arm for `ws_topic::ENCRYPTION`
- This caused `subscribe_to_topics()` to return early with an error when subscribing to the "encryption" topic
- The encryption topic was never added to the subscription guard
- As a result, `encryption.session_ready` WS events were silently dropped for all subscribers

**Fix applied** (ws.rs, added before the `unsupported =>` catch-all):
```rust
ws_topic::ENCRYPTION => {
    // No snapshot for encryption — only an event is emitted on session_ready.
    Ok(None)
}
```

After the fix, TC4 WS test confirmed `encryption.session_ready` event is delivered correctly to subscribers.

### Precondition Handling

Encryption initialization was done via the setup flow (POST /setup/host → /setup/submit-passphrase) since the HTTP handlers don't initialize encryption — only unlock/lock/query existing state. The daemon vault (`.app_data/vault/`) is in `src-tauri/.app_data/` due to relative path resolution from config.toml, not `~/Library/Application Support/`.

### Daemon Restart Limitation

When the vault contains an initialized encryption state (`.initialized_encryption` present), the daemon's `auto_unlock_encryption_session` use case attempts keyring-based recovery on startup. This fails if the KEK is not in the macOS keychain with the correct passphrase, causing the daemon to exit with "wrong passphrase". This is expected behavior — for UAT, encryption state was reset between test runs by removing `.initialized_encryption`.

### Failure Signals Verified

- No 500 errors observed on any endpoint during UAT
- WS events are properly delivered after fix

### Token Auth

- Daemon requires `Authorization: Session <jwt>` header (not `Bearer`)
- Bearer token for `/auth/connect` sourced from `/tmp/uniclipboard-daemon.token`
- Session token obtained via POST /auth/connect with valid bearer token + PID
