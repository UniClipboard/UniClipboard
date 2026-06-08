//! Daemon-side broadcaster for the stateless setup pairing events
//! (Slice4 Phase 3).
//!
//! Fans `SpaceSetupFacade` lifecycle outcomes onto the `setup` ws topic.
//! The forwarder spawned by [`spawn_pairing_completion_forwarder`] is the
//! glue between the application-layer `PairingOutcome` broadcast and the
//! daemon-wide ws event bus.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use uc_application::facade::PairingOutcome;
use uc_daemon_contract::api::dto::setup_events::{
    SetupInvitationIssuedEvent, SetupInvitationRevokedEvent, SetupPairingCompletedEvent,
};
use uc_daemon_contract::api::types::DaemonWsEvent;
use uc_daemon_contract::constants::{ws_event, ws_topic};

/// Fans out setup pairing lifecycle events onto the daemon ws bus
/// under `ws_topic::SETUP`.
#[derive(Clone)]
pub struct SetupEventBroadcaster {
    event_tx: broadcast::Sender<DaemonWsEvent>,
}

impl SetupEventBroadcaster {
    pub fn new(event_tx: broadcast::Sender<DaemonWsEvent>) -> Self {
        Self { event_tx }
    }

    pub fn emit_invitation_issued(&self, code: String, expires_at_ms: i64) {
        self.send(
            ws_event::SETUP_INVITATION_ISSUED,
            SetupInvitationIssuedEvent {
                code,
                expires_at_ms,
            },
        );
    }

    pub fn emit_pairing_completed(
        &self,
        sponsor_device_id: String,
        joiner_device_id: Option<String>,
        success: bool,
        reason: Option<String>,
    ) {
        self.send(
            ws_event::SETUP_PAIRING_COMPLETED,
            SetupPairingCompletedEvent {
                sponsor_device_id,
                joiner_device_id,
                success,
                reason,
            },
        );
    }

    pub fn emit_invitation_revoked(&self, reason: String) {
        self.send(
            ws_event::SETUP_INVITATION_REVOKED,
            SetupInvitationRevokedEvent { reason },
        );
    }

    fn send<T: Serialize>(&self, event_type: &'static str, payload: T) {
        let payload = match serde_json::to_value(payload) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(error = %error, event_type, "failed to serialize setup ws event");
                return;
            }
        };

        let _ = self.event_tx.send(DaemonWsEvent {
            topic: ws_topic::SETUP.to_string(),
            event_type: event_type.to_string(),
            session_id: None,
            ts: chrono::Utc::now().timestamp_millis(),
            payload,
        });
    }
}

/// Bridge sponsor-side `PairingOutcome` events onto the daemon ws bus as
/// `setup.pairingCompleted` envelopes.
///
/// Spawns a single tokio task that lives until `cancel` fires or the
/// upstream broadcast `Sender` drops (which lands as `Closed` on the
/// receiver). `Lagged` is logged but never aborts the loop — the next
/// outcome still fans out.
///
/// `sponsor_device_id` is captured at spawn time and stamped on every
/// outgoing envelope so the frontend can attribute the event without
/// having to thread it through the application layer.
///
/// Takes a raw `Receiver<PairingOutcome>` rather than the facade so the
/// loop is unit-testable without the full setup-deps assembly.
pub fn spawn_pairing_completion_forwarder(
    mut outcome_rx: broadcast::Receiver<PairingOutcome>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    sponsor_device_id: String,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    let broadcaster = Arc::new(SetupEventBroadcaster::new(event_tx));
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    debug!("pairing-completion forwarder cancelled");
                    break;
                }
                recv = outcome_rx.recv() => match recv {
                    Ok(PairingOutcome::Success { peer_device_id, .. }) => {
                        broadcaster.emit_pairing_completed(
                            sponsor_device_id.clone(),
                            Some(peer_device_id.to_string()),
                            true,
                            None,
                        );
                    }
                    Ok(PairingOutcome::Failure { reason }) => {
                        // `Display` of `PairingFailureReason` is the
                        // stable `snake_case` identifier — same wire
                        // form as the analytics event so dashboard and
                        // pairing-completed event payload stay aligned.
                        broadcaster.emit_pairing_completed(
                            sponsor_device_id.clone(),
                            None,
                            false,
                            Some(reason.to_string()),
                        );
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(
                            skipped = n,
                            "pairing-completion forwarder lagged — some events dropped"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!(
                            "pairing-completion forwarder: facade broadcast closed, exiting"
                        );
                        break;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    fn setup() -> (SetupEventBroadcaster, broadcast::Receiver<DaemonWsEvent>) {
        let (tx, rx) = broadcast::channel(8);
        (SetupEventBroadcaster::new(tx), rx)
    }

    #[tokio::test]
    async fn invitation_issued_routes_to_setup_topic_with_camel_case_payload() {
        let (broadcaster, mut rx) = setup();

        broadcaster.emit_invitation_issued("ABCD-1234".to_string(), 1_745_577_600_000);
        let event = rx.recv().await.expect("event delivered");

        assert_eq!(event.topic, "setup");
        assert_eq!(event.event_type, "setup.invitationIssued");
        assert!(event.session_id.is_none());
        assert_eq!(event.payload["code"], "ABCD-1234");
        assert_eq!(event.payload["expiresAtMs"], 1_745_577_600_000_i64);
        assert!(event.payload.get("expires_at_ms").is_none());
    }

    #[tokio::test]
    async fn pairing_completed_carries_both_device_ids_and_reason() {
        let (broadcaster, mut rx) = setup();

        broadcaster.emit_pairing_completed(
            "sponsor-1".to_string(),
            Some("joiner-2".to_string()),
            false,
            Some("timeout".to_string()),
        );
        let event = rx.recv().await.expect("event delivered");

        assert_eq!(event.topic, "setup");
        assert_eq!(event.event_type, "setup.pairingCompleted");
        assert_eq!(event.payload["sponsorDeviceId"], "sponsor-1");
        assert_eq!(event.payload["joinerDeviceId"], "joiner-2");
        assert_eq!(event.payload["success"], false);
        assert_eq!(event.payload["reason"], "timeout");
    }

    #[tokio::test]
    async fn pairing_completed_success_carries_null_reason() {
        let (broadcaster, mut rx) = setup();

        broadcaster.emit_pairing_completed(
            "sponsor-1".to_string(),
            Some("joiner-2".to_string()),
            true,
            None,
        );
        let event = rx.recv().await.expect("event delivered");

        assert_eq!(event.payload["success"], true);
        assert!(event.payload["reason"].is_null());
    }

    #[tokio::test]
    async fn pairing_completed_failure_without_joiner_id_carries_null_field() {
        let (broadcaster, mut rx) = setup();

        broadcaster.emit_pairing_completed(
            "sponsor-1".to_string(),
            None,
            false,
            Some("proof_mismatch".to_string()),
        );
        let event = rx.recv().await.expect("event delivered");

        assert_eq!(event.payload["sponsorDeviceId"], "sponsor-1");
        assert!(event.payload["joinerDeviceId"].is_null());
        assert_eq!(event.payload["success"], false);
        assert_eq!(event.payload["reason"], "proof_mismatch");
    }

    #[tokio::test]
    async fn invitation_revoked_carries_reason() {
        let (broadcaster, mut rx) = setup();

        broadcaster.emit_invitation_revoked("cancelled".to_string());
        let event = rx.recv().await.expect("event delivered");

        assert_eq!(event.topic, "setup");
        assert_eq!(event.event_type, "setup.invitationRevoked");
        assert_eq!(event.payload["reason"], "cancelled");
    }

    #[tokio::test]
    async fn ts_is_populated_with_recent_wallclock() {
        let (broadcaster, mut rx) = setup();
        let before = chrono::Utc::now().timestamp_millis();

        broadcaster.emit_invitation_revoked("expired".to_string());
        let event = rx.recv().await.expect("event delivered");
        let after = chrono::Utc::now().timestamp_millis();

        assert!(
            event.ts >= before && event.ts <= after,
            "ts={} not in [{}, {}]",
            event.ts,
            before,
            after
        );
    }

    // ── pairing-completion forwarder ───────────────────────────────────────

    mod forwarder {
        use super::*;
        use std::time::Duration;
        use uc_application::facade::PairingFailureReason;
        use uc_core::ids::DeviceId;
        use uc_core::security::IdentityFingerprint;

        fn fp() -> IdentityFingerprint {
            IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap()
        }

        async fn next_event(rx: &mut broadcast::Receiver<DaemonWsEvent>) -> Option<DaemonWsEvent> {
            tokio::time::timeout(Duration::from_millis(500), rx.recv())
                .await
                .ok()
                .and_then(Result::ok)
        }

        #[tokio::test]
        async fn success_outcome_emits_pairing_completed_with_joiner_id() {
            let (outcome_tx, outcome_rx) = broadcast::channel(8);
            let (event_tx, mut event_rx) = broadcast::channel(8);
            let cancel = CancellationToken::new();

            let handle = spawn_pairing_completion_forwarder(
                outcome_rx,
                event_tx,
                "sponsor-A".to_string(),
                cancel.clone(),
            );

            outcome_tx
                .send(PairingOutcome::Success {
                    peer_device_id: DeviceId::new("joiner-B"),
                    peer_device_name: "Mac".to_string(),
                    peer_fingerprint: fp(),
                })
                .expect("forwarder should keep an active receiver");

            let event = next_event(&mut event_rx).await.expect("event delivered");
            assert_eq!(event.topic, "setup");
            assert_eq!(event.event_type, "setup.pairingCompleted");
            assert_eq!(event.payload["sponsorDeviceId"], "sponsor-A");
            assert_eq!(event.payload["joinerDeviceId"], "joiner-B");
            assert_eq!(event.payload["success"], true);
            assert!(event.payload["reason"].is_null());

            cancel.cancel();
            let _ = handle.await;
        }

        #[tokio::test]
        async fn failure_outcome_emits_pairing_completed_with_null_joiner_and_reason() {
            let (outcome_tx, outcome_rx) = broadcast::channel(8);
            let (event_tx, mut event_rx) = broadcast::channel(8);
            let cancel = CancellationToken::new();

            let handle = spawn_pairing_completion_forwarder(
                outcome_rx,
                event_tx,
                "sponsor-A".to_string(),
                cancel.clone(),
            );

            outcome_tx
                .send(PairingOutcome::Failure {
                    reason: PairingFailureReason::PassphraseMismatch,
                })
                .expect("forwarder receiver active");

            let event = next_event(&mut event_rx).await.expect("event delivered");
            assert_eq!(event.event_type, "setup.pairingCompleted");
            assert_eq!(event.payload["sponsorDeviceId"], "sponsor-A");
            assert!(event.payload["joinerDeviceId"].is_null());
            assert_eq!(event.payload["success"], false);
            // `Display` produces the snake_case wire form — same identifier
            // dashboards see in `pairing_failed.failure_reason`.
            assert_eq!(event.payload["reason"], "passphrase_mismatch");

            cancel.cancel();
            let _ = handle.await;
        }

        #[tokio::test]
        async fn cancel_token_stops_forwarder_promptly() {
            let (_outcome_tx, outcome_rx) = broadcast::channel::<PairingOutcome>(8);
            let (event_tx, _event_rx) = broadcast::channel(8);
            let cancel = CancellationToken::new();

            let handle = spawn_pairing_completion_forwarder(
                outcome_rx,
                event_tx,
                "sponsor".to_string(),
                cancel.clone(),
            );

            cancel.cancel();
            tokio::time::timeout(Duration::from_millis(500), handle)
                .await
                .expect("forwarder should exit within 500ms after cancel")
                .expect("task should join cleanly");
        }

        #[tokio::test]
        async fn upstream_close_exits_forwarder() {
            let (outcome_tx, outcome_rx) = broadcast::channel::<PairingOutcome>(8);
            let (event_tx, _event_rx) = broadcast::channel(8);
            let cancel = CancellationToken::new();

            let handle = spawn_pairing_completion_forwarder(
                outcome_rx,
                event_tx,
                "sponsor".to_string(),
                cancel,
            );

            // Drop the upstream sender — the receiver should observe Closed
            // and the loop should exit on its own without cancellation.
            drop(outcome_tx);

            tokio::time::timeout(Duration::from_millis(500), handle)
                .await
                .expect("forwarder should exit within 500ms after upstream close")
                .expect("task should join cleanly");
        }

        #[tokio::test]
        async fn lagged_outcome_is_logged_and_loop_keeps_running() {
            // Capacity 1: send 3 outcomes back-to-back to force the forwarder
            // to lag past the buffer; a fourth send must still reach it.
            let (outcome_tx, outcome_rx) = broadcast::channel(1);
            let (event_tx, mut event_rx) = broadcast::channel(8);
            let cancel = CancellationToken::new();

            let handle = spawn_pairing_completion_forwarder(
                outcome_rx,
                event_tx,
                "sponsor".to_string(),
                cancel.clone(),
            );

            // Block the forwarder from draining for a moment; pile up sends.
            for _ in 0..3 {
                let _ = outcome_tx.send(PairingOutcome::Failure {
                    reason: PairingFailureReason::Internal,
                });
            }
            // The forwarder may have observed Lagged on the first recv.
            // A subsequent send must still be delivered.
            outcome_tx
                .send(PairingOutcome::Success {
                    peer_device_id: DeviceId::new("late-joiner"),
                    peer_device_name: "Late".to_string(),
                    peer_fingerprint: fp(),
                })
                .expect("forwarder still alive");

            // Drain ws events until we see the success — earlier failure
            // events from before the lag may also land; we only assert the
            // forwarder eventually delivers the final success.
            let mut seen_success = false;
            for _ in 0..6 {
                match next_event(&mut event_rx).await {
                    Some(event) if event.payload["joinerDeviceId"] == "late-joiner" => {
                        seen_success = true;
                        break;
                    }
                    Some(_) => continue,
                    None => break,
                }
            }
            assert!(
                seen_success,
                "forwarder must keep running after a Lagged error"
            );

            cancel.cancel();
            let _ = handle.await;
        }
    }
}
