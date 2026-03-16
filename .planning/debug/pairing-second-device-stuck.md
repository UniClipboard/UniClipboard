---
status: fix_applied
trigger: 'pairing-second-device-stuck'
created: 2026-03-15
updated: 2026-03-16T00:00:00Z
---

## Current Focus

hypothesis: CONFIRMED — SpaceAccessOrchestrator is a singleton shared across all pairing sessions. After Device B completes space access, the state machine reaches terminal `Granted` state. When Device C pairs and the sponsor tries `start_sponsor_authorization`, the state machine silently ignores the event (terminal catch-all), so no offer is ever sent to Device C.
test: cargo check passes, all space_access tests pass (7 in uc-core, 19 in uc-app)
expecting: Second device should now receive the sponsor's offer and complete space access
next_action: Await human verification

## Symptoms

expected: When a second device enters the correct verification key during pairing with a sponsor, the pairing should complete successfully.
actual: The second device reaches the key verification step normally but entering the correct key produces no response. The UI is stuck. Only restarting both devices allows re-pairing.
errors: From Seq logs (15:57):

- Pairing itself succeeds: "Handling pairing confirm message" → "Emitting pairing result to frontend"
- Space access fails: "waiting for joiner offer before starting join space access" → "start join space access requested without received offer"
  reproduction: 1. Start Device A as sponsor. 2. Pair Device B with Device A and complete full setup. 3. Attempt to pair Device C with Device A. 4. Device C reaches key verification. 5. Enter correct key. 6. No response, stuck.
  started: 2026-03-15

## Root Cause

The `SpaceAccessOrchestrator` (singleton, `Arc<SpaceAccessOrchestrator>`) uses a state machine that only allows `SponsorAuthorizationRequested` from the `Idle` state. Terminal states (`Granted`, `Denied`, `Cancelled`) have a catch-all that silently ignores all events.

After Device B's space access completes → state = `Granted`. When Device C pairs and the sponsor's wiring calls `start_sponsor_authorization` → state machine receives `SponsorAuthorizationRequested` in `Granted` state → catch-all returns (Granted, []) → no `SendOffer` action → Device C never receives offer → timeout.

## Fix

1. **State machine** (`uc-core/src/security/space_access/state_machine.rs`):
   Added explicit transitions from `Granted | Denied | Cancelled` + `SponsorAuthorizationRequested` → `WaitingJoinerProof` with full offer preparation actions. Placed before the terminal catch-all so they match first.

2. **Orchestrator** (`uc-app/src/usecases/space_access/orchestrator.rs`):
   Added context cleanup in `dispatch()` when transitioning from a terminal state — clears `prepared_offer`, `joiner_offer`, `joiner_passphrase`, `proof_artifact`, `result_success`, `result_deny_reason`. Preserves `sponsor_peer_id` (set by wiring before dispatch).

3. **Tests**: Added 3 new tests verifying re-authorization from each terminal state.

## Files Changed

- `src-tauri/crates/uc-core/src/security/space_access/state_machine.rs`
- `src-tauri/crates/uc-app/src/usecases/space_access/orchestrator.rs`
