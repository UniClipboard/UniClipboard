# Phase 59: Secure daemon resource endpoints - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-25
**Phase:** 59-secure-daemon-resource-endpoints
**Areas discussed:** extraction scope, HTTP type dependency, daemon reuse scenario, security strategy, rate limiting, GUI token auth, token refresh

---

## Extraction Scope

| Option                      | Description                                                                                          | Selected |
| --------------------------- | ---------------------------------------------------------------------------------------------------- | -------- |
| Types + parse logic         | UcRoute, UcRequestError + parse function move to uc-core, parse signature changed to accept &str URI |          |
| Types only                  | UcRoute + UcRequestError to uc-core, parse stays in uc-tauri                                         |          |
| Full + resolve              | Types, parse, and resolve all extracted with trait abstraction                                       |          |
| **Scheme C: No extraction** | daemon uses axum native routes, no shared URI parsing needed                                         | ✓        |

**User's choice:** Scheme C — don't extract. The `uc://` cross-platform URI format handling is Tauri WebView-specific; daemon as a standard HTTP server doesn't need it.
**Notes:** User requested to step back from extraction and consider fundamentally better approaches. After analyzing that `uc://` protocol parsing is a Tauri-only concern, agreed that daemon should use standard REST routes.

---

## Security Strategy

| Option               | Description                                      | Selected |
| -------------------- | ------------------------------------------------ | -------- |
| Reuse existing auth  | Bearer token auth same as management endpoints   |          |
| Enhanced auth        | Additional protection layers beyond bearer token | ✓        |
| Defer implementation | Record requirements only, implement later        |          |

**User's choice:** Enhanced auth — resource endpoints serve raw clipboard content (potentially passwords, screenshots), requiring higher security than management endpoints.
**Notes:** User emphasized future `0.0.0.0` support as the design constraint. Security must not rely on localhost isolation.

---

## Security Measures Selected

All four options selected plus additional requirements:

| Measure                            | Selected |
| ---------------------------------- | -------- |
| Bearer + rate limit                | ✓        |
| Short-term access tokens           | ✓        |
| Origin + session check             | ✓        |
| Additional (all stricter measures) | ✓        |

**User's notes:** "更严格" — wants maximum security posture. All plaintext reads must enter audit log. Token scope must be isolated from general API permissions.

---

## Rate Limiting

| Option                                       | Description           | Selected |
| -------------------------------------------- | --------------------- | -------- |
| Recommended: thumbnail 100/s, blob 20/s      | Per-token rate limits | ✓        |
| More conservative: thumbnail 30/s, blob 10/s | Lower limits          |          |
| Claude decides                               | Based on testing      |          |

**User's choice:** Recommended limits — thumbnail 100 req/s, blob 20 req/s per token.

---

## GUI Authentication Method

| Option            | Description                                          | Selected |
| ----------------- | ---------------------------------------------------- | -------- |
| Signed URL        | Frontend batch-signs URLs with HMAC query params     |          |
| Tauri proxy layer | Keep uc:// handler as proxy, Tauri backend adds auth | ✓        |
| HttpOnly Cookie   | Daemon issues session cookie                         |          |

**User's choice:** Tauri proxy layer — frontend zero changes, security handled entirely in Rust backend.
**Notes:** User specified: (1) Tauri must NOT use reqwest directly, must use daemon-client; (2) Token 15-second TTL, lazy refresh on expiry.

---

## Token Refresh Timing

| Option            | Description                                 | Selected |
| ----------------- | ------------------------------------------- | -------- |
| Lazy refresh      | Check expiry on request, refresh if expired | ✓        |
| Proactive renewal | Background timer refreshes before expiry    |          |
| Claude decides    | Based on complexity                         |          |

**User's choice:** Lazy refresh — simple and reliable, no background timer needed.

---

## GUI Route (after daemon endpoints ready)

| Option           | Description                                   | Selected |
| ---------------- | --------------------------------------------- | -------- |
| Keep as-is       | Tauri continues using in-process uc://        |          |
| Switch to daemon | GUI loads resources via daemon HTTP endpoints | ✓        |
| Gradual switch   | Keep current, switch in later phase           |          |

**User's choice:** Switch to daemon — but via Tauri proxy layer, so frontend still uses `uc://` URIs transparently.

---

## Claude's Discretion

- Token signing mechanism (HMAC, JWT, or opaque)
- Audit log storage format and location
- axum middleware structure for rate limiting and auth
- Content-Disposition strategy per resource type
- Error response format for 401/403/429

## Deferred Ideas

- Actual `0.0.0.0` binding (future phase)
- TLS for daemon HTTP server
- Cross-device resource access
