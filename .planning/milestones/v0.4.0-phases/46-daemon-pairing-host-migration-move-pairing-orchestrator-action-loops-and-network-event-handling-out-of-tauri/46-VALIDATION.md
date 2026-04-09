---
phase: 46
slug: daemon-pairing-host-migration-move-pairing-orchestrator-action-loops-and-network-event-handling-out-of-tauri
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-19
---

# Phase 46 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                                                                                                    |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Framework**          | Rust `cargo test`                                                                                                                                        |
| **Config file**        | `src-tauri/Cargo.toml`                                                                                                                                   |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon --test pairing_host --test pairing_api --test pairing_ws -p uc-tauri --test pairing_bridge -- --test-threads=1` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-daemon -p uc-tauri -- --test-threads=1`                                                                                |
| **Estimated runtime**  | ~120 seconds                                                                                                                                             |

---

## Sampling Rate

- **After every task commit:** Run the task-local command from the verification map below
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-daemon -p uc-tauri -- --test-threads=1`
- **Before `$gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type            | Automated Command                                                                                                                                 | File Exists | Status     |
| -------- | ---- | ---- | ----------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| 46-01-01 | 01   | 1    | PH46-01     | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_host -- daemon_pairing_host_enforces_single_active_session --test-threads=1`              | ❌ W0       | ⬜ pending |
| 46-01-02 | 01   | 1    | PH46-01A/B  | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_host -- daemon_pairing_host_starts_non_discoverable_in_headless_mode --test-threads=1`    | ❌ W0       | ⬜ pending |
| 46-01-03 | 01   | 1    | PH46-02     | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_host -- daemon_pairing_host_survives_client_disconnect --test-threads=1`                  | ❌ W0       | ⬜ pending |
| 46-02-01 | 02   | 2    | PH46-03     | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_api -- pairing_mutations_ack_immediately_and_require_session_id --test-threads=1`         | ❌ W0       | ⬜ pending |
| 46-02-02 | 02   | 2    | PH46-03     | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_api -- pairing_api_returns_409_active_pairing_session_exists --test-threads=1`            | ❌ W0       | ⬜ pending |
| 46-02-03 | 02   | 2    | PH46-03     | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_api -- pairing_api_returns_412_when_no_local_participant_ready --test-threads=1`          | ❌ W0       | ⬜ pending |
| 46-02-04 | 02   | 2    | PH46-03A    | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_api -- pairing_api_returns_409_host_not_discoverable --test-threads=1`                    | ❌ W0       | ⬜ pending |
| 46-02-05 | 02   | 2    | PH46-03A    | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_api -- pairing_api_requires_explicit_discoverability_opt_in_for_cli --test-threads=1`     | ❌ W0       | ⬜ pending |
| 46-02-06 | 02   | 2    | PH46-03A    | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_api -- pairing_api_expires_discoverability_lease --test-threads=1`                        | ❌ W0       | ⬜ pending |
| 46-02-07 | 02   | 2    | PH46-03     | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_api -- pairing_api_returns_404_for_unknown_followup_session --test-threads=1`             | ❌ W0       | ⬜ pending |
| 46-02-08 | 02   | 2    | PH46-03     | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_api -- pairing_api_returns_400_for_malformed_payload --test-threads=1`                    | ❌ W0       | ⬜ pending |
| 46-02-09 | 02   | 2    | PH46-04     | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_ws -- pairing_ws_hides_verification_secrets_from_summary_payloads --test-threads=1`       | ❌ W0       | ⬜ pending |
| 46-02-10 | 02   | 2    | PH46-03A    | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_ws -- pairing_ws_emits_peers_name_and_connection_incremental_events --test-threads=1`     | ❌ W0       | ⬜ pending |
| 46-02-11 | 02   | 2    | PH46-03A    | integration          | `cd src-tauri && cargo test -p uc-daemon --test pairing_ws -- pairing_ws_peer_payloads_are_camelcase_and_bridge_compatible --test-threads=1`      | ❌ W0       | ⬜ pending |
| 46-03-01 | 03   | 3    | PH46-05     | integration          | `cd src-tauri && cargo test -p uc-tauri --test pairing_bridge -- bridge_preserves_existing_pairing_event_contract --test-threads=1`               | ❌ W0       | ⬜ pending |
| 46-03-02 | 03   | 3    | PH46-05A    | integration          | `cd src-tauri && cargo test -p uc-tauri --test pairing_bridge -- bridge_emits_lease_lost_degradation_event --test-threads=1`                      | ❌ W0       | ⬜ pending |
| 46-04-01 | 04   | 4    | PH46-06     | compile + unit       | `cd src-tauri && cargo test -p uc-bootstrap --lib -- --test-threads=1 && cargo check -p uc-app`                                                   | ❌ W0       | ⬜ pending |
| 46-05-01 | 05   | 5    | PH46-06     | integration          | `cd src-tauri && cargo test -p uc-tauri --test pairing_bridge -- bridge_feeds_setup_with_setup_pairing_facade --test-threads=1`                   | ❌ W0       | ⬜ pending |
| 46-05-02 | 05   | 5    | PH46-06     | integration + manual | `cd src-tauri && cargo test -p uc-tauri --test pairing_bridge -- bridge_keeps_setup_flow_semantics --test-threads=1`                              | ❌ W0       | ⬜ pending |
| 46-05-03 | 05   | 5    | PH46-05A    | integration          | `cd src-tauri && cargo test -p uc-tauri --test pairing_bridge -- bridge_registers_gui_client_as_discoverable_by_default --test-threads=1`         | ❌ W0       | ⬜ pending |
| 46-05-04 | 05   | 5    | PH46-05A    | integration          | `cd src-tauri && cargo test -p uc-tauri --test pairing_bridge -- bridge_sets_participant_ready_only_when_pairing_flow_is_active --test-threads=1` | ❌ W0       | ⬜ pending |
| 46-05-05 | 05   | 5    | PH46-05A    | integration          | `cd src-tauri && cargo test -p uc-tauri --test pairing_bridge -- bridge_revokes_discoverability_and_ready_on_shutdown --test-threads=1`           | ❌ W0       | ⬜ pending |
| 46-05-06 | 05   | 5    | PH46-05A    | integration          | `cd src-tauri && cargo test -p uc-tauri --test pairing_bridge -- bridge_reports_lease_loss_without_dropping_active_session --test-threads=1`      | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-daemon/tests/pairing_host.rs` — host ownership, single-session gate, and disconnect continuity coverage
- [ ] `src-tauri/crates/uc-daemon/tests/pairing_api.rs` — daemon pairing mutation/control surface coverage, including explicit discoverability opt-in for CLI
- [ ] `src-tauri/crates/uc-daemon/tests/pairing_ws.rs` — realtime contract, discovery incremental events, and secret-boundary coverage
- [ ] `src-tauri/crates/uc-tauri/tests/pairing_bridge.rs` — Tauri compatibility bridge contract coverage, including setup facade binding, GUI discoverability/readiness lifecycle, and lease-loss handling

---

## Manual-Only Verifications

| Behavior                                                                                                    | Requirement          | Why Manual                                                         | Test Instructions                                                                                                                                                                                                                                                                      |
| ----------------------------------------------------------------------------------------------------------- | -------------------- | ------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Headless daemon stays out of discovery until a CLI user explicitly enters pairing mode                      | PH46-01A/B, PH46-03A | Requires two real runtimes and peer discovery visibility check     | Start a daemon-only host without GUI and without any CLI pairing-mode command, confirm a second device cannot discover it; then run the explicit CLI pairing-mode command and confirm the host becomes discoverable before attempting pairing                                          |
| CLI pairing-mode lease expires and host becomes non-discoverable again                                      | PH46-03A             | Requires waiting for lease timeout and re-checking discovery       | Enable CLI pairing mode with a short lease, confirm host becomes discoverable, wait for lease expiry (or stop command), then confirm host is removed from peer discovery without manual daemon restart                                                                                 |
| Daemon-kept session outlives Tauri/webview disconnect but UI does not auto-resume it                        | PH46-02              | Requires real desktop shell lifecycle and reconnect timing         | Start pairing from desktop app, reach verification or waiting state, restart or detach the Tauri/webview host, confirm daemon session remains active until timeout or terminal result, then reopen UI and confirm there is no automatic session-resume UX in Phase 46                  |
| Verification codes and fingerprints remain absent from normal session reads                                 | PH46-04              | Requires cross-checking realtime path against HTTP session summary | Start a pairing flow, call `/pairing/sessions/{sessionId}` through an authenticated local client, confirm response omits short code and fingerprints, then inspect authenticated realtime pairing updates to confirm verification data is delivered only there                         |
| GUI-hosted daemon remains discoverable by default while participant-ready toggles with active pairing flows | PH46-05A             | Requires desktop shell startup and pairing-flow lifecycle check    | Start GUI app and confirm host is discoverable; before opening pairing UI/setup flow verify it is not marked participant-ready for inbound acceptance; open pairing UI/flow and confirm ready is asserted; close flow or exit app and confirm discoverability/ready are revoked        |
| GUI lease loss prevents new inbound pairing but does not silently drop an already active pairing session    | PH46-05A             | Requires induced bridge/lease failure during active pairing        | Start a GUI-driven pairing flow, force the bridge lease renewal to fail while a session is already active, confirm daemon stops advertising new participant availability, confirm current session still reaches terminal result, and confirm UI surfaces the degraded state explicitly |
| Setup flow still reaches the expected pairing-confirm/join-space transitions through the daemon bridge      | PH46-06              | Requires end-to-end setup navigation in desktop shell              | Launch desktop app in setup flow, join a peer through the existing UI, confirm stage transitions still follow `request -> verification -> verifying -> complete/failed` semantics and join-space path is not skipped or duplicated                                                     |

---

## Validation Sign-Off

- [ ] All tasks have `<acceptance_criteria>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
