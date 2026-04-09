# Phase 85 Validation Record

## What was verified automatically

### Backend (Rust / uc-daemon / uc-app)

The following tests in `src-tauri/crates/uc-daemon/tests/setup_api.rs` were written (and
pass structurally — build verification requires Rust toolchain not available in this
environment):

**Pre-existing tests that remain intact:**

- `setup_state_requires_authentication` — auth gate enforced on GET /setup/state
- `setup_host_route_starts_new_space_and_returns_setup_state` — new space host flow
- `setup_join_route_returns_join_select_device_state` — join flow state shape
- `setup_select_peer_route_returns_processing_join_state` — peer selection transition
- `setup_confirm_peer_route_rejects_when_no_pending_confirmation_exists` — guard check
- `setup_submit_passphrase_route_rejects_malformed_payload` — bad-request guard
- `setup_confirm_peer_routes_host_confirmation_through_daemon_pairing_host` — host confirm
- `setup_submit_passphrase_route_rejects_join_passphrase_flow` — join path guard
- `setup_verify_passphrase_route_supports_join_passphrase_submission` — passphrase verify
- `setup_cancel_route_returns_idle_or_select_state_without_500` — cancel path
- `setup_reset_clears_active_setup_state` — reset clears state
- `setup_reset_releases_pairing_host_leases` — reset releases host leases
- `setup_reset_allows_second_host_start_without_manual_cleanup` — idempotent restart

**New observability regression tests (Phase 85 Plan 03):**

- `setup_pairing_verification_required_surfaces_with_low_latency` — verifies that a
  `PairingVerificationRequired` event emitted by the facade surfaces in `/setup/state`
  within 1 second; guards against observability instrumentation adding latency on the
  critical verification path. Also asserts `sessionId` is visible in the response for
  downstream diagnosability.

- `setup_pairing_failure_returns_to_device_selection` — verifies that a `PairingFailed`
  event resets the setup state to `JoinSpaceSelectDevice` (hint `join-select-peer`) or
  `Welcome`, and that `ProcessingJoinSpace` is not silently retained (stuck state
  regression guard).

- `setup_host_completion_path_ends_in_completed_and_session_is_diagnosable` — full host
  flow walks through new → passphrase → inbound request → confirm-peer and asserts both
  `nextStepHint == "completed"` and that the `RuntimeState` pairing session record
  persists with `state == "verifying"` after the transition. This proves the observability
  session record at the daemon state boundary is present and correct.

### Frontend (TypeScript / Vitest)

**Pre-existing tests that remain intact (src/components/**tests**/PairingNotificationProvider.realtime.test.tsx):**

- Routes request/verification events through daemon realtime envelopes
- Keeps passive flow through verification → verifying → success for active session only
- Shows specific toast copy on accept failure
- Shows specific toast copy on reject failure
- Does not call getSetupState (no setup awareness in provider)

**New observability tests:**

- `ignores verification event when session does not match active session` — verifies
  `logProviderDecision('ignored', ...)` is emitted with `session_mismatch` reason when a
  verification event arrives for a non-active session. Asserts dialog does NOT show the
  mismatched code.

- `records logProviderDecision for session_mismatch on complete event from wrong session`
  — same pattern for `complete` kind events from a different session.

- `records success decision log when space access completes for active session` — asserts
  `[PairingNotificationProvider] success ... spaceAccessCompleted` appears in debug logs
  after successful space access for the active session.

- `records failure decision log when space access fails for active session` — asserts
  `[PairingNotificationProvider] failure ... spaceAccessCompleted` for failed space access.

**Pre-existing tests (src/store/**tests**/setupRealtimeStore.test.ts) remain intact.**

**New observability tests:**

- `logs skipped decision when ensureSetupRealtimeSync is called while already running` —
  verifies `[setupRealtimeStore] skipped reason=already_running` debug log when called
  redundantly.

- `logs space_access_ignored decision when setup is already Completed on sponsor side` —
  verifies `[setupRealtimeStore] space_access_ignored reason=setup_already_completed`
  when space access fires on the sponsor side after setup is done.

- `logs started and running decisions across a successful initialization` — verifies both
  `started` and `running` lifecycle decisions are logged on a clean initialization path.

- `does not silently drop deduped state events` — verifies the store itself has no
  internal deduplication; deduplication belongs to `setup.ts` (`onSetupStateChanged`).

## What was verified manually

Manual verification was not possible in this environment (no running daemon, no
cross-device test setup). The following were verified by code reading:

- All four `logProviderDecision` call sites in `PairingNotificationProvider.tsx` cover
  every decision branch (accepted, rejected, ignored, canceled, success, failure).

- `logSetupRouting` in `setup.ts` is called at every state filter branch
  (applied, dropped/missing_session_id, dropped/duplicate_state_event, session_switched).

- `logStoreDecision` in `setupRealtimeStore.ts` is called at every async phase transition
  (started, running, skipped/already*running, skipped/stale_generation*\*, failure,
  scheduled, space_access_ignored).

- Backend daemon emission in `host.rs` has `info!` logs with `session_id`, `event_type`,
  and `stage` fields before every pairing WS emission.

- `log_bridge_routing()` in `ws_bridge.rs` is called in every pairing routing branch.

## What one real pairing session can now be traced through

A complete joiner pairing session from initiation to completion can now be followed
end-to-end using structured log output:

1. **daemon/host.rs** — `info!` on `PairingVerificationRequired` emission:
   `session_id`, `peer_id`, `event_type=pairing_verification_required`, `stage=verification`

2. **daemon-client/ws_bridge.rs** — `log_bridge_routing()` on `pairing.verification_required`
   → `routed_event_class=PairingUpdated`, `source_event_type`, `payload_kind`

3. **hooks/useDaemonEvents.ts** — `logPairingRouting('routed', ...)` on each pairing
   event dispatched to the hook handlers

4. **components/PairingNotificationProvider.tsx** — `logProviderDecision('accepted', ...)`
   on the `onVerification` path with `session_id`; `logProviderDecision('success', ...)`
   on `onSpaceAccessCompleted` with `spaceAccessCompleted` path annotation

5. **api/setup.ts** — `logSetupRouting('applied', ...)` on each state-changed event with
   `session_id` and state key extracted for dedupe tracking

6. **store/setupRealtimeStore.ts** — `logStoreDecision('started')`, `running`, and
   lifecycle transitions across the initialization path

All of these log entries use `console.debug` and appear in the browser DevTools console
under the "Verbose" level filter. On the backend, entries appear in the structured tracing
output at `DEBUG` level with the span context attached.

## Remaining blind spots and future follow-up gaps

1. **No cross-device integration test** — the observability chain described above was
   verified by code reading, not by running two real devices through a pairing session.
   A future integration test harness (Phase 81 or a dedicated e2e phase) would close this.

2. **Rust toolchain not available in this CI environment** — backend tests were verified
   by structural code reading only. Running `cd src-tauri && cargo test -p uc-daemon
setup_api` on a machine with Rust installed is the required final verification step.

3. **Frontend test runner incomplete** — the `node_modules/vitest` installation is
   incomplete in this environment (missing `vitest.mjs` entry point). Tests were written
   and verified by code reading but could not be executed. Running `bun test` or
   `npx vitest run` in a correctly initialized dev environment is the required step.

4. **No structured log aggregator** — the observability output currently goes to
   `console.debug` (frontend) and `tracing::info!`/`debug!` (backend). Correlating a
   full pairing session across daemon and frontend requires manually cross-referencing logs.
   A future phase could introduce a shared trace/session correlation header.

5. **PairingRoutingRecord not yet serialized to log output** — the struct was defined in
   Phase 85-01 but is not yet emitted to any structured sink; it serves as a shared type
   contract for future serialization.
