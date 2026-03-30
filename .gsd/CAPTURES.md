# Captures

### CAP-8a3f1c2d

**Text:** UAT Workflow: Daemon HTTP API (M002-zldd9y/S02 — Settings & Encryption HTTP Handlers)
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending

---

### CAP-1b4e9a7f

**Text:** Daemon vault is at `src-tauri/.app_data/vault/`, not `~/Library/Application Support/uniclipboard/`. Relative paths resolve from `config.toml` location (`src-tauri/`).
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending

### CAP-2c5d8b3e

**Text:** Key files in vault: `.initialized_encryption` (determines initialized: true/false), `keyslot.json` (master key wrapping material)
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending

### CAP-3d6e9c4f

**Text:** Precondition for clean state: Encryption must be `Uninitialized` for daemon to start without keyring recovery. To reset: `rm -f src-tauri/.app_data/vault/.initialized_encryption src-tauri/.app_data/vault/keyslot.json`
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending

### CAP-4e7f0d5a

**Text:** Bearer token location: `/tmp/uniclipboard-daemon.token`, format: 64-char hex (HS256 secret for JWT signing)
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending

### CAP-5f8a1e6b

**Text:** Auth header: `Authorization: Bearer <token>` for `/auth/connect` only. All other endpoints require `Authorization: Session <jwt>` (not Bearer).
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending

### CAP-6a9b2f7c

**Text:** To initialize encryption (for unlock/lock tests): POST /setup/reset → POST /setup/host → POST /setup/submit-passphrase. After this, GET /encryption/state shows initialized: true, sessionReady: true.
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending

### CAP-7b0c3a8d

**Text:** Bug found: WS `encryption.session_ready` events silently dropped. Root cause in `src-tauri/crates/uc-daemon/src/api/ws.rs`: `build_snapshot_event()` had no match arm for `ws_topic::ENCRYPTION`. Fix: add `ws_topic::ENCRYPTION => Ok(None)` before `unsupported =>`.
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending

### CAP-8c1d4b9e

**Text:** Error response format: `{"error":{"code":"<snake_case_code>","message":"<human-readable>"}}`. Known codes: wrong_passphrase (401), not_initialized (400), bad_request (400), internal_error (500), invalid_session_token (401), rate_limit_exceeded (429)
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending

### CAP-9d2e5c0f

**Text:** Daemon HTTP base: `http://127.0.0.1:42715` (auto-assigned port, from logs). Health check: GET /health (no auth). WS: `ws://127.0.0.1:42715/ws`.
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending

### CAP-a3f4d1e2

**Text:** Template for future UAT: bash script at end of CAPTURES.md documents daemon startup, auth, and HTTP API testing workflow. Reusable for regression testing.
**Captured:** 2026-03-30T02:21:48.000Z
**Status:** pending
