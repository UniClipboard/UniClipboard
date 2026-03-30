# UAT Workflow Capture — S02 Daemon HTTP API

**Date:** 2026-03-30
**Slice:** M002-zldd9y/S02 (Settings & Encryption HTTP Handlers)
**Mode:** Live-runtime (required — HTTP handlers need real daemon + runtime state)

---

## Workflow Phases

### Phase 1 — Daemon Vault Discovery

The daemon vault is **NOT** in `~/Library/Application Support/uniclipboard/`.

It is at `src-tauri/.app_data/vault/` because:
- `config.toml` has `vault_key_path = ".app_data/vault/key"`
- Relative paths resolve from config.toml location (`src-tauri/`)
- So vault dir = `src-tauri/.app_data/vault/`

Key files in vault:
- `.initialized_encryption` — exists when encryption is initialized (determines `initialized: true/false`)
- `keyslot.json` — master key wrapping material

### Phase 2 — Daemon Startup (Clean State)

**Precondition:** Encryption must be `Uninitialized` for daemon to start without keyring recovery.

Steps:
```bash
# 1. Kill any running daemon
pkill -f uniclipboard-daemon

# 2. Remove vault state
rm -f src-tauri/.app_data/vault/.initialized_encryption
rm -f src-tauri/.app_data/vault/keyslot.json

# 3. Start daemon with --dev
./src-tauri/target/release/uniclipboard-daemon --dev
# Wait for: "HTTP server listening" in logs
```

If `.initialized_encryption` exists on startup, daemon attempts `auto_unlock_encryption_session()` which loads the KEK from macOS keychain and unwraps the master key. If the passphrase doesn't match, it crashes with:
```
Cannot start daemon: encryption session recovery failed: failed to unwrap master key: wrong passphrase
```

### Phase 3 — Authentication

**Bearer token location:** `/tmp/uniclipboard-daemon.token`
**Bearer token format:** 64-char hex (HS256 secret for JWT signing)

**Auth flow:**
```bash
# Get JWT session token
TOKEN=$(curl -s -X POST http://127.0.0.1:42715/auth/connect \
  -H "Authorization: Bearer $BEARER_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"pid\":$RANDOM,\"clientType\":\"cli\"}" \
  | jq -r '.sessionToken')
```

**Critical:** HTTP header uses `Authorization: Session $TOKEN` (not `Bearer`).
`Bearer` prefix is for `/auth/connect` only. All other endpoints require `Session`.

### Phase 4 — Encryption Initialization (for unlock tests)

The HTTP handlers don't initialize encryption — they only unlock/lock/query. To test unlock/lock:
```bash
# Reset setup state
curl -s -X POST -H "Authorization: Session $TOKEN" \
  -H "Content-Type: application/json" -d '{}' \
  http://127.0.0.1:42715/setup/reset

# Host a space
curl -s -X POST -H "Authorization: Session $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"deviceName":"TestDevice"}' \
  http://127.0.0.1:42715/setup/host

# Submit passphrase → initializes encryption
curl -s -X POST -H "Authorization: Session $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"passphrase":"testpass123"}' \
  http://127.0.0.1:42715/setup/submit-passphrase
```

After this, GET /encryption/state shows `initialized: true, sessionReady: true`.

### Phase 5 — HTTP API Testing

```bash
# GET /settings → 200 {data: {...}, ts: <millis>}
curl -s -H "Authorization: Session $TOKEN" http://127.0.0.1:42715/settings | jq .

# PUT /settings partial merge → 200
curl -s -X PUT -H "Authorization: Session $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"general":{"device_name":"Test"}}' \
  http://127.0.0.1:42715/settings | jq .

# GET /encryption/state → 200 {data: {initialized, sessionReady}}
curl -s -H "Authorization: Session $TOKEN" http://127.0.0.1:42715/encryption/state | jq .

# POST /encryption/unlock wrong passphrase → 401
curl -s -X POST -H "Authorization: Session $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"passphrase":"wrong"}' \
  http://127.0.0.1:42715/encryption/unlock

# POST /encryption/lock → 200
curl -s -X POST -H "Authorization: Session $TOKEN" \
  -H "Content-Type: application/json" -d '{}' \
  http://127.0.0.1:42715/encryption/lock

# POST /encryption/unlock correct → 200
curl -s -X POST -H "Authorization: Session $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"passphrase":"testpass123"}' \
  http://127.0.0.1:42715/encryption/unlock
```

### Phase 6 — WebSocket Testing

Requires Node.js with `ws` package.

```bash
# Lock first (so we have a state transition to observe)
curl -s -X POST -H "Authorization: Session $TOKEN" \
  -H "Content-Type: application/json" -d '{}' \
  http://127.0.0.1:42715/encryption/lock > /dev/null

# Node.js WS test
node << 'EOF'
const WebSocket = require('ws');
const execSync = require('child_process').execSync;

const ws = new WebSocket('ws://127.0.0.1:42715/ws', {
  headers: { 'Authorization': 'Session ' + process.env.TOKEN }
});

ws.on('open', () => {
  ws.send(JSON.stringify({action:'subscribe', topics:['encryption']}));
});

ws.on('message', d => {
  const msg = d.toString();
  console.log('MSG:', msg.substring(0, 200));
  if (msg.includes('encryption.session_ready')) {
    console.log('PASS: encryption.session_ready event!');
    ws.close();
    process.exit(0);
  }
});

ws.on('error', e => { console.error('ERR:', e.message); process.exit(1); });

// After 500ms, trigger unlock
setTimeout(() => {
  const r = execSync(
    'curl -s -X POST -H "Authorization: Session ' + process.env.TOKEN + '" ' +
    '-H "Content-Type: application/json" ' +
    '-d \'{"passphrase":"testpass123"}\' ' +
    'http://127.0.0.1:42715/encryption/unlock'
  );
  console.log('Unlock:', r.toString());
}, 500);

setTimeout(() => {
  console.log('TIMEOUT - no encryption.session_ready event');
  process.exit(1);
}, 5000);
EOF
```

### Phase 7 — Cleanup

```bash
pkill -f uniclipboard-daemon
rm -f src-tauri/.app_data/vault/.initialized_encryption
rm -f src-tauri/.app_data/vault/keyslot.json
```

---

## Bug Found During UAT

### WS `encryption.session_ready` events silently dropped

**File:** `src-tauri/crates/uc-daemon/src/api/ws.rs`

**Root cause:** `build_snapshot_event()` had no match arm for `ws_topic::ENCRYPTION`. When a client subscribed to the "encryption" topic:
1. `subscribe_to_topics()` called `build_snapshot_event(state, "encryption")`
2. The match hit `unsupported => anyhow::bail!("unsupported websocket topic: {unsupported}")`
3. The error caused `subscribe_to_topics()` to return early without adding "encryption" to the subscription set
4. Fan-out loop never matched the topic → events were silently dropped

**Note:** `is_supported_topic()` DID include `ENCRYPTION` (so the subscription wasn't rejected), but the missing match arm in `build_snapshot_event()` caused the subscription to fail silently.

**Fix applied:**
```rust
// In build_snapshot_event() match block, before `unsupported =>`:
ws_topic::ENCRYPTION => {
    // No snapshot for encryption — only an event is emitted on session_ready.
    Ok(None)
}
```

---

## Error Response Format

All error responses use:
```json
{
  "error": {
    "code": "<snake_case_code>",
    "message": "<human-readable>"
  }
}
```

Known codes:
- `wrong_passphrase` → 401 Unauthorized
- `not_initialized` → 400 Bad Request
- `bad_request` → 400 Bad Request (malformed JSON)
- `internal_error` → 500 Internal Server Error
- `invalid_session_token` → 401 Unauthorized (missing/malformed JWT)
- `rate_limit_exceeded` → 429 Too Many Requests

---

## Port and URL Notes

- Daemon HTTP base: `http://127.0.0.1:42715` (auto-assigned port, from logs)
- Health check: `GET /health` (no auth required)
- All other endpoints require `Authorization: Session <jwt>`
- WS: `ws://127.0.0.1:42715/ws` (uses same auth header)

---

## Template for Future UAT

```bash
#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="..."  # adjust
DAEMON="$PROJECT_ROOT/src-tauri/target/release/uniclipboard-daemon"
VAULT_DIR="$PROJECT_ROOT/src-tauri/.app_data/vault"
TOKEN_FILE="/tmp/uniclipboard-daemon.token"
PORT=42715

cleanup() {
  pkill -f uniclipboard-daemon || true
  rm -f "$VAULT_DIR/.initialized_encryption" "$VAULT_DIR/keyslot.json" || true
}

start_daemon() {
  $DAEMON --dev > /tmp/daemon.log 2>&1 &
  for i in $(seq 1 40); do
    lsof -i :$PORT | grep -q LISTEN && return 0
    sleep 0.5
  done
  echo "Daemon failed to start"; cat /tmp/daemon.log; exit 1
}

get_token() {
  curl -s -X POST "http://127.0.0.1:$PORT/auth/connect" \
    -H "Authorization: Bearer $(cat $TOKEN_FILE)" \
    -H "Content-Type: application/json" \
    -d "{\"pid\":$$,\"clientType\":\"cli\"}" | jq -r '.sessionToken'
}

cleanup
start_daemon
TOKEN=$(get_token)

# Now run tests...
curl -s -H "Authorization: Session $TOKEN" "http://127.0.0.1:$PORT/settings"
```
