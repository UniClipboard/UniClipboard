# S02: Settings & Encryption HTTP Handlers — UAT

**Milestone:** M002-zldd9y
**Written:** 2026-03-30T01:24:21.659Z

## UAT Type
- UAT mode: Live-runtime (requires a running daemon with an active encryption session)
- Why this mode is sufficient: Pure HTTP API handlers — correctness depends on runtime behavior (settings persistence, encryption state, WS events)

## Preconditions
1. Daemon is running with a valid JWT session token in Authorization: Bearer <token> header
2. Encryption has been initialized (device has key material on disk)
3. A WebSocket connection is open to the daemon to observe WS broadcast events

## Smoke Test
```bash
# GET /settings — should return 200 with settings data
curl -s -H "Authorization: Bearer <token>" http://localhost:41891/settings | jq .
# GET /encryption/state — should return 200 with {initialized, sessionReady}
curl -s -H "Authorization: Bearer <token>" http://localhost:41891/encryption/state | jq .
```

## Test Cases

### 1. GET /settings returns current settings
1. Send GET /settings with valid auth
2. Expected: 200 OK, body {"data": {...}, "ts": <millis>}

### 2. PUT /settings partial update merges with existing settings
1. Send PUT /settings with {"autoLockMinutes": 30}
2. Expected: 200 OK
3. GET /settings confirms only autoLockMinutes changed; others unchanged

### 3. POST /encryption/unlock with wrong passphrase returns 401
1. Send POST /encryption/unlock with {"passphrase": "wrongpass"}
2. Expected: 401 Unauthorized, code: "wrong_passphrase"

### 4. POST /encryption/unlock with correct passphrase returns 200 and broadcasts WS event
1. Send POST /encryption/unlock with correct passphrase
2. Expected: 200 OK
3. Expected WS event: topic="encryption", eventType="encryption.session_ready"

### 5. POST /encryption/lock clears session
1. Unlock first, then POST /encryption/lock
2. Expected: 200 OK; GET /encryption/state shows sessionReady: false

## Edge Cases
- Malformed JSON in PUT /settings → 400 Bad Request
- Empty passphrase in unlock → 401 (treated as wrong passphrase)
- Uninitialized encryption unlock attempt → 400 NotInitialized

## Failure Signals
- 500 on any endpoint indicates internal error in handler logic
- WS event not received after unlock → broadcast channel issue

## Not Proven By This UAT
- OS-level side effects (autostart, keyboard shortcuts) — intentionally omitted from HTTP handler
- L3/L4 permission enforcement — deferred to future phases
- Settings persistence across daemon restart — tested in S03
