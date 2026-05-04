# Direct Daemon WebSocket — UAT Runbook

**Date**: 2026-03-30
**Phase**: M003 S07 — Direct Daemon WS & Integration Proof Remediation
**Scope**: Browser↔Daemon WebSocket path, bearer→session auth, reconnect recovery

---

## Overview

This runbook verifies the end-to-end WebSocket path from the browser (React frontend) to the daemon HTTP/WS server (`uc-daemon`). The path was migrated from Tauri `invoke()` to direct HTTP + WebSocket in M003.

### What This UAT Covers

| Claim | How Verified |
|-------|-------------|
| Bearer token exchanges for JWT session token | `POST /auth/connect` succeeds with 200 |
| Browser-compatible WS auth via `?auth=Session%20TOKEN` | WS opens with 101 status |
| Snapshot events delivered over WS | Clipboard snapshot received within 10s |
| Reconnect recovery | Disconnect + reconnect succeeds |
| No raw tokens in diagnostics | Proof harness redacts all secrets |

### What This UAT Does NOT Cover

- L3 encryption enforcement (Phase 76 scope)
- End-to-end P2P clipboard sync (Phase 76+ scope)
- macOS system clipboard integration

---

## Prerequisites

1. **Daemon must be running** with a valid `daemon.token` file
2. **`scripts/verify-direct-daemon-ws.mjs`** must exist and be executable
3. **Node.js ≥ 18** (for native `fetch` and `WebSocket`)
4. **Known daemon base URL and bearer token**

### Finding the Daemon Token

The bearer token is stored in the daemon's data directory:

```bash
# Find the token file
find ~/Library/Application\ Support/uniclipboard -name "daemon.token" 2>/dev/null

# Read the token (keep it private!)
cat ~/Library/Application\ Support/uniclipboard/daemon.token
```

### Finding the Daemon Port

The daemon listens on an ephemeral port (chosen at startup). The Tauri app emits it via `daemon://connection-info`:

```bash
# Look for the daemon port in recent logs
grep -r "listening" ~/Library/Application\ Support/uniclipboard/logs/ 2>/dev/null | tail -5

# Or look for the HTTP server binding
grep -r "127.0.0.1:" ~/Library/Application\ Support/uniclipboard/logs/ 2>/dev/null | grep -v "442\|443" | tail -10
```

---

## Running the Proof Harness

### Self-Test Mode (No Live Daemon Required)

Verifies internal consistency of the proof harness logic:

```bash
node scripts/verify-direct-daemon-ws.mjs --self-test
```

**Expected output:**
```
============================================================
VERIFICATION: Direct Daemon WS — Self-Test Mode
============================================================

[AUTH        ] Simulating bearer→session exchange...
[AUTH        ] ✅ Session exchange shape valid (expiresInSecs=300)
[WS_OPEN     ] Testing WebSocket URL with ?auth= query param...
[WS_OPEN     ] ✅ WS URL construction valid (base=ws://127.0.0.1:42715/ws)
[SUBSCRIBE   ] Testing subscribe message envelope...
[SUBSCRIBE   ] ✅ Subscribe envelope valid (topics=clipboard,peers)
[SNAPSHOT    ] Testing event envelope parsing...
[SNAPSHOT    ] ✅ Event envelope parsing valid (topic=clipboard, eventType=clipboard.new-content)
[RECONNECT   ] Testing reconnect delay bounds...
[RECONNECT   ] ✅ Reconnect delay bounds valid (max=30000ms, attempts=10)

------------------------------------------------------------
SELF-TEST RESULT: 5 passed, 0 failed out of 5 checks
------------------------------------------------------------

  ✅ auth_exchange_shape
  ✅ ws_url_construction
  ✅ subscribe_envelope
  ✅ event_envelope_parse
  ✅ reconnect_delay_bounds

✅ All self-tests passed. Proof harness internal consistency verified.
```

**Exit code**: 0 on success, 1 on failure

---

### Live Mode (Against Running Daemon)

Requires a running daemon with valid credentials:

```bash
DAEMON_BASE_URL=http://127.0.0.1:<port> \
DAEMON_TOKEN=$(cat ~/Library/Application\ Support/uniclipboard/daemon.token) \
DAEMON_PID=$(pgrep -f "uniclipboard" | head -1) \
node scripts/verify-direct-daemon-ws.mjs --live
```

**Example with real values:**
```bash
DAEMON_BASE_URL=http://127.0.0.1:42715 \
DAEMON_TOKEN=3f4a9c2e1b7d... \
node scripts/verify-direct-daemon-ws.mjs --live
```

**Expected output:**
```
============================================================
VERIFICATION: Direct Daemon WS — Live Mode
============================================================

[CONFIG      ] DAEMON_BASE_URL=http://127.0.0.1:42715
[CONFIG      ] DAEMON_PID=12345
[CONFIG      ] ✅ Config valid (host=127.0.0.1:42715)
[AUTH        ] Exchanging bearer→session...
[AUTH        ]    POST http://127.0.0.1:42715/auth/connect
[AUTH        ] ✅ Auth success (sessionToken=[redacted], expiresIn=300s, latency=12ms)
[WS_OPEN     ] Opening WebSocket...
[WS_OPEN     ] ✅ WebSocket open (latency=8ms)
[SUBSCRIBE   ] Subscribing to clipboard topic...
[SUBSCRIBE   ] ✅ Subscribe sent (nonce=x7k2m9n1)
[SNAPSHOT    ] Waiting for snapshot event (timeout=10000ms)...
[SNAPSHOT    ]    Received: topic=clipboard, eventType=clipboard.new-content
[SNAPSHOT    ] ✅ Snapshot received (payload keys: entry_id, preview, origin)
[RECONNECT   ] Testing disconnect/reconnect...
[RECONNECT   ] ✅ Reconnect successful

------------------------------------------------------------
LIVE MODE RESULT: ✅ All stages passed
------------------------------------------------------------

Evidence:
  ✅ Auth exchange: bearer→session (expiresIn=300s)
  ✅ WebSocket open: /ws with ?auth=Session%20<token>
  ✅ Subscribe: clipboard topic sent
  ✅ Snapshot: received before timeout
  ✅ Reconnect: disconnected and reconnected successfully

✅ Direct daemon WebSocket path verified end-to-end.
```

**Exit codes:**
| Code | Meaning |
|------|---------|
| 0 | All checks passed |
| 1 | Configuration error |
| 2 | Auth failure (401, invalid token) |
| 3 | WebSocket handshake failure |
| 4 | Timeout (no snapshot/event received) |
| 5 | Malformed response |

---

## Inspecting Auth Failures

### 401 Unauthorized

**Symptom**: Auth exchange fails with 401

**Causes**:
1. Bearer token is expired or invalid
2. Daemon was restarted and the token file was regenerated
3. Token file permissions are wrong (should be `chmod 600`)

**Resolution**:
```bash
# Regenerate token by restarting the daemon (the app does this automatically)
# Check token file permissions
ls -la ~/Library/Application\ Support/uniclipboard/daemon.token
# Should show: -rw------- (600)

# Get fresh token
cat ~/Library/Application\ Support/uniclipboard/daemon.token
```

### Invalid Session Token (WS 401)

**Symptom**: WebSocket opens but immediately fails with 401

**Cause**: Session token expired (TTL 300s) — you waited too long before connecting

**Resolution**: Retry immediately; the script gets a fresh session before connecting

### PID Not Allowed (WS 403)

**Symptom**: Auth succeeds but WS fails with 403 `pid_not_allowed`

**Cause**: The PID in the `/auth/connect` body doesn't match a registered PID

**Resolution**: Ensure `DAEMON_PID` is set to the actual GUI process PID, not a shell PID

---

## Inspecting Reconnect Failures

### Max Reconnect Attempts Exceeded

**Symptom**: Client retries 10 times then gives up

**Daemon log evidence**:
```
[DaemonWsClient] gave up after 10 reconnect attempts
```

**Likely causes**:
1. Daemon crashed and didn't restart
2. Rate limiting triggered (101st request in 60s)
3. Network policy blocking loopback

### Stale Session on Reconnect

**Symptom**: Reconnect succeeds but no snapshot/event received

**Cause**: Session token expired between disconnect and reconnect

**Resolution**: Implement session refresh on reconnect (planned for Phase 76)

---

## Verifying Browser Console Output

When running the app in dev mode, open the browser console (F12 → Console) and filter for `[DaemonWsClient]`:

```
[DaemonWsClient] scheduling reconnect attempt 1/10 in 1000ms
[DaemonWsClient] scheduling reconnect attempt 2/10 in 2000ms
...
```

**Good signs**:
- `[DaemonWsClient] WebSocket open` — connection established
- `[DaemonWsClient] Received: clipboard.new-content` — snapshot received

**Bad signs**:
- `[DaemonWsClient] gave up after 10 reconnect attempts` — reconnect exhausted
- `[DaemonWsClient] failed to handle incoming message` — malformed envelope

---

## Verifying Security Properties

### Token Redaction Check

The proof harness must NOT print raw bearer or session tokens:

```bash
# Run live mode and capture output
DAEMON_BASE_URL=http://127.0.0.1:42715 \
DAEMON_TOKEN=test-secret-abc123 \
node scripts/verify-direct-daemon-ws.mjs --live 2>&1 | grep -i token

# Should NOT show: test-secret-abc123
# Should show: [redacted], [redacted], or 4-char...12 chars...last4
```

### Rate Limiting Check

```bash
# Send 101 rapid auth requests — the 101st should get 429
for i in $(seq 1 101); do
  STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "Authorization: Bearer $(cat ~/Library/Application\ Support/uniclipboard/daemon.token)" \
    -H "Content-Type: application/json" \
    -d '{"pid":1234,"clientType":"gui"}' \
    http://127.0.0.1:42715/auth/connect)
  echo "Request $i: HTTP $STATUS"
done

# Expected: first 100 → 200, 101st → 429
```

---

## Adding New Test Cases

When adding new WebSocket topics or event types:

1. Add the topic to `scripts/verify-direct-daemon-ws.mjs` in the `runLiveMode` subscribe section
2. Add the expected envelope shape to the self-test (Test 4)
3. Update this runbook with the new topic name

Example — adding `encryption` topic:
```javascript
// In runLiveMode():
const subscribeMsg = {
  action: 'subscribe',
  topics: ['clipboard', 'encryption'],  // ← add here
  nonce: Math.random().toString(36).slice(2, 10),
}
```

---

## CI Integration

Add to your CI pipeline:

```yaml
# .github/workflows/verify.yml
- name: Verify direct daemon WS
  run: |
    # Start daemon (or assume it's already running)
    bun run tauri dev &
    DAEMON_PID=$!
    
    # Wait for daemon to be ready
    sleep 5
    
    # Run proof harness
    node scripts/verify-direct-daemon-ws.mjs --self-test
    
    # Cleanup
    kill $DAEMON_PID 2>/dev/null
```

---

## Related Documents

- `docs/security-audit.md` — Security audit with token handling, rate limiting, PID verification
- `src/api/daemon/client.ts` — HTTP client (session exchange)
- `src/lib/daemon-ws.ts` — WebSocket client (subscribe, reconnect)
- `src-tauri/crates/uc-daemon/src/api/ws.rs` — Daemon WS handler
- `src-tauri/crates/uc-daemon/src/api/auth.rs` — Daemon auth endpoint
