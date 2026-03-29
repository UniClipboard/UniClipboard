# Phase 81: Frontend-Daemon Integration Testing & Security Audit - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning
**Source:** PRD Express Path (docs/plans/frontend-direct-daemon-connection.md)

<domain>
## Phase Boundary

End-to-end integration tests for the full frontend-daemon direct connection stack: HTTP API correctness, WebSocket event delivery, session token lifecycle, reconnection recovery, and security audit covering token leakage, rate limiting effectiveness, and permission enforcement.

</domain>

<decisions>
## Implementation Decisions

### Integration Test Coverage

- HTTP API correctness: all clipboard, settings, encryption, storage endpoints return correct data
- WebSocket event delivery: events reach frontend within expected latency
- Session token lifecycle: refresh, expiry, re-auth flow works end-to-end
- Reconnection recovery: data consistency after WS disconnect/reconnect
- Permission enforcement: L1-L4 levels correctly gate endpoints
- Rate limiting: verify 100 req/min limit triggers 429 responses

### Security Audit Checklist

- Bearer token not exposed in frontend logs or network waterfall
- Session token not persisted to localStorage/sessionStorage
- CORS headers prevent cross-origin access (no CORS needed for localhost same-origin)
- PID verification rejects requests from unknown processes
- Rate limiting prevents rapid enumeration attacks
- L3 endpoints reject requests when encryption session not ready
- L4 endpoints require explicit confirmation
- No sensitive data in URL query parameters

### Test Infrastructure

- Frontend tests: Vitest with mock daemon server
- Backend tests: Rust integration tests with real HTTP client
- End-to-end: test daemon + frontend interaction scenarios

### Performance Baseline

- HTTP API response time < 50ms for list endpoints
- WebSocket event delivery < 100ms end-to-end
- Session refresh overhead < 200ms

### Claude's Discretion

- Test framework choices for E2E testing
- Mock vs real daemon for frontend tests
- Security audit tooling and methodology
- Performance test harness design
- CI integration approach

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Test Infrastructure

- `src/` — Frontend test patterns (Vitest, jsdom)
- `src-tauri/crates/uc-daemon/tests/` — Daemon integration test patterns

### All Previous Phase Outputs

- Phase 74: Daemon clipboard HTTP API
- Phase 75: Security middleware
- Phase 76: Settings/encryption/storage API
- Phase 77: Frontend HTTP client
- Phase 78: Clipboard API migration
- Phase 79: WebSocket direct connection
- Phase 80: Command cleanup

### Security References

- PRD Section 4: Security Architecture (threat model, layer design)
- PRD Section 7: Risk matrix

</canonical_refs>

<specifics>
## Specific Ideas

- PRD success criteria: 90%+ API calls through daemon HTTP/WS, 60%+ command code reduction, security audit pass
- Token leakage test: verify bearer token appears only in Authorization header, never in logs
- Rate limit test: send 101 requests in <1 minute, verify 429 on 101st
- Permission test: call L3 endpoint without encryption session, verify ENCRYPTION_NOT_READY error
- Reconnection test: kill daemon, restart, verify frontend auto-reconnects and refreshes data

</specifics>

<deferred>
## Deferred Ideas

- Automated security scanning tools integration
- Performance regression test suite
- Cross-platform testing (Windows, Linux-specific edge cases)

</deferred>

---

_Phase: 81-frontend-daemon-integration-testing_
_Context gathered: 2026-03-29 via PRD Express Path_
