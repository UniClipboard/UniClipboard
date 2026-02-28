use std::sync::Arc;

use tokio::sync::Mutex;
use uc_core::network::{PairingBusy, PairingMessage, SessionId};
use uc_core::ports::space::SpaceAccessTransportPort;
use uc_core::ports::PairingTransportPort;
use uc_core::security::space_access::deny_reason_to_code;

use super::context::SpaceAccessContext;

pub struct SpaceAccessNetworkAdapter {
    network: Arc<dyn PairingTransportPort>,
    context: Arc<Mutex<SpaceAccessContext>>,
}

impl SpaceAccessNetworkAdapter {
    pub fn new(
        network: Arc<dyn PairingTransportPort>,
        context: Arc<Mutex<SpaceAccessContext>>,
    ) -> Self {
        Self { network, context }
    }
}

#[async_trait::async_trait]
impl SpaceAccessTransportPort for SpaceAccessNetworkAdapter {
    /// Uses `PairingMessage::Busy` as a temporary envelope in `send_pairing_on_session` for
    /// space-access protocol messages, storing JSON in `PairingBusy.reason` with shape
    /// `{"kind":"space_access_offer|space_access_proof|space_access_result", ...}`.
    /// TODO: replace this with a dedicated `PairingMessage::SpaceAccess` variant.
    async fn send_offer(&mut self, session_id: &SessionId) -> anyhow::Result<()> {
        let offer = {
            let context = self.context.lock().await;
            context
                .prepared_offer
                .clone()
                .ok_or_else(|| anyhow::anyhow!("missing prepared_offer in space access context"))?
        };

        let payload = serde_json::json!({
            "kind": "space_access_offer",
            "space_id": offer.space_id.as_str(),
            "nonce": offer.nonce,
            "keyslot": offer.keyslot,
        });
        let payload = serde_json::to_string(&payload)?;

        self.network
            .send_pairing_on_session(PairingMessage::Busy(PairingBusy {
                session_id: session_id.to_string(),
                reason: Some(payload),
            }))
            .await
    }

    async fn send_proof(&mut self, session_id: &SessionId) -> anyhow::Result<()> {
        let proof = {
            let context = self.context.lock().await;
            context
                .proof_artifact
                .clone()
                .ok_or_else(|| anyhow::anyhow!("missing proof_artifact in space access context"))?
        };

        let payload = serde_json::json!({
            "kind": "space_access_proof",
            "pairing_session_id": proof.pairing_session_id.as_str(),
            "space_id": proof.space_id.as_str(),
            "challenge_nonce": proof.challenge_nonce,
            "proof_bytes": proof.proof_bytes,
        });
        let payload = serde_json::to_string(&payload)?;

        self.network
            .send_pairing_on_session(PairingMessage::Busy(PairingBusy {
                session_id: session_id.to_string(),
                reason: Some(payload),
            }))
            .await
    }

    async fn send_result(&mut self, session_id: &SessionId) -> anyhow::Result<()> {
        let payload = {
            let context = self.context.lock().await;
            let space_id = context
                .prepared_offer
                .as_ref()
                .map(|offer| offer.space_id.as_str().to_string())
                .or_else(|| {
                    context
                        .joiner_offer
                        .as_ref()
                        .map(|offer| offer.space_id.as_str().to_string())
                })
                .or_else(|| {
                    context
                        .proof_artifact
                        .as_ref()
                        .map(|proof| proof.space_id.as_str().to_string())
                })
                .ok_or_else(|| anyhow::anyhow!("missing space_id in space access context"))?;

            let success = context
                .result_success
                .ok_or_else(|| anyhow::anyhow!("missing result_success in space access context"))?;

            let deny_reason = context.result_deny_reason.as_ref().map(deny_reason_to_code);

            serde_json::json!({
                "kind": "space_access_result",
                "space_id": space_id,
                "sponsor_peer_id": context.sponsor_peer_id.clone(),
                "success": success,
                "deny_reason": deny_reason,
            })
        };
        let payload = serde_json::to_string(&payload)?;

        self.network
            .send_pairing_on_session(PairingMessage::Busy(PairingBusy {
                session_id: session_id.to_string(),
                reason: Some(payload),
            }))
            .await
    }
}
