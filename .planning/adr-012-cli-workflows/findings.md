# Findings

## Baseline

- Engine is pinned to `94b21aac9db2fa0fb89bc9027f0e05e545ecc1f5`.
- CLI implements default join waiting, no-wait return, status, and cancel while keeping Engine state authoritative.
- daemon already exposes current device trust, trust decision, cancel join, and member sync preference endpoints.
- Rust daemon client exposes cancel join, trust decision, and member sync preference methods.
- The `member` CLI includes trust and sync workflow groups alongside removal.

## Invariants

- Human-facing CLI text is English; project documentation is Chinese.
- JSON/non-interactive trust decisions bind to an expected change ID.
- Ctrl-C detaches from a pending join and never cancels it.
- No client-side cancel/reset/join orchestration.
- While a join operation is active, `/health` can report `degraded` because receive-readiness queries are temporarily unavailable. The daemon contract is still compatible and setup control APIs remain usable.
- The pinned Engine keeps the durable current join visible while the session transition temporarily owns other profile state. This makes a deterministic real-daemon `pending` fixture possible without CLI-owned state.
- Real-daemon coverage proves pending-state observation, Ctrl-C detachment, daemon restart visibility, explicit cancellation, trust decisions, and sync read-update-read behavior.
- Setup control may attach to a temporarily degraded daemon only when the package version and API revision match and the health reason is exactly `degraded`; matching versions alone are insufficient.

## Completion audit

| ADR requirement | Current evidence | Audit result |
| --- | --- | --- |
| Default wait, Ctrl-C detaches, status remains queryable | Real pending E2E observes the same join ID before and after Ctrl-C | Proven |
| Explicit cancel and empty cancel | Real E2E plus exact daemon-client route coverage | Proven |
| CLI and daemon restart preserve authoritative status | Real active and pending joins remain visible after daemon restart | Proven |
| Trust status/apply/keep/local removal/stale change | Three-daemon E2E plus focused checks in human and JSON modes | Proven |
| Sync show/partial set/reread | Two-daemon E2E in human and JSON modes plus controlled reread-failure coverage | Proven |
| Human and JSON state matrix | Real workflows plus focused output-shape and selection tests | Proven |
| One expected user action per command | Real-daemon request-log deltas plus exact HTTP client tests | Proven |

- Engine `94b21aac9db2fa0fb89bc9027f0e05e545ecc1f5` returns the durable current join even when trust details are temporarily unavailable during a session transition. The commit is available on remote branch `fix/current-join-visible-during-transition`.
- Passphrase mismatch is rejected by the sponsor before Candidate and does not leave a public rejected projection on the joiner. The CLI correctly reports the immediate request failure; subsequent `join status` is `none`.
- A status query observes rejected state and should succeed. A cancel action succeeds only when Engine returns either pending with `cancel_requested=true` or rejected with reason `cancelled`; active or unrelated rejection is a cancellation failure.
