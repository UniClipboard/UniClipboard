//! Setup pairing WebSocket event payloads (Slice4 Phase 3 T3.1).
//!
//! These payloads ride on `ws_topic::SETUP` and replace the legacy
//! `setup.stateChanged` / `setup.spaceAccessCompleted` projections that
//! belonged to the stateful `SetupFacade`. The new `SpaceSetupFacade`
//! emits stateless lifecycle events instead.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Payload for `ws_event::SETUP_INVITATION_ISSUED`.
///
/// Emitted on the sponsor side immediately after
/// `SpaceSetupFacade::issue_pairing_invitation()` succeeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupInvitationIssuedEvent {
    /// Human-typeable invitation code shown to the joiner.
    pub code: String,
    /// Wall-clock expiry of the invitation in milliseconds since epoch.
    pub expires_at_ms: i64,
}

/// Payload for `ws_event::SETUP_PAIRING_COMPLETED`.
///
/// Both sides receive this when the pairing handshake terminates.
/// `success = false` carries a `reason` describing the failure mode.
///
/// `joiner_device_id` is `None` when the handshake fails before the
/// joiner identity is committed (e.g. proof verification failed before
/// `PersistPairedDevice`); on success it is always populated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupPairingCompletedEvent {
    /// Device id of the sponsor (the side that issued the invitation).
    pub sponsor_device_id: String,
    /// Device id of the joiner. `None` on early failures before the
    /// joiner identity is observed.
    pub joiner_device_id: Option<String>,
    /// Whether the handshake succeeded.
    pub success: bool,
    /// Failure reason when `success = false`. `None` on success.
    pub reason: Option<String>,
}

/// Payload for `ws_event::SETUP_INVITATION_REVOKED`.
///
/// Emitted when an in-flight invitation is cancelled by the user or
/// expires before any joiner redeems it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupInvitationRevokedEvent {
    /// Reason the invitation was revoked (e.g. `"cancelled"`, `"expired"`).
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_issued_serializes_camel_case() {
        let event = SetupInvitationIssuedEvent {
            code: "ABCD-1234".to_string(),
            expires_at_ms: 1_745_577_600_000,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["code"], "ABCD-1234");
        assert_eq!(json["expiresAtMs"], 1_745_577_600_000_i64);
        assert!(json.get("expires_at_ms").is_none());
    }

    #[test]
    fn pairing_completed_serializes_camel_case_with_reason() {
        let event = SetupPairingCompletedEvent {
            sponsor_device_id: "sponsor-1".to_string(),
            joiner_device_id: Some("joiner-2".to_string()),
            success: false,
            reason: Some("timeout".to_string()),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["sponsorDeviceId"], "sponsor-1");
        assert_eq!(json["joinerDeviceId"], "joiner-2");
        assert_eq!(json["success"], false);
        assert_eq!(json["reason"], "timeout");
    }

    #[test]
    fn pairing_completed_omits_none_reason_as_null() {
        let event = SetupPairingCompletedEvent {
            sponsor_device_id: "sponsor-1".to_string(),
            joiner_device_id: Some("joiner-2".to_string()),
            success: true,
            reason: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert!(json["reason"].is_null());
    }

    #[test]
    fn pairing_completed_failure_carries_null_joiner_id() {
        let event = SetupPairingCompletedEvent {
            sponsor_device_id: "sponsor-1".to_string(),
            joiner_device_id: None,
            success: false,
            reason: Some("proof_mismatch".to_string()),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["sponsorDeviceId"], "sponsor-1");
        assert!(json["joinerDeviceId"].is_null());
        assert_eq!(json["success"], false);
        assert_eq!(json["reason"], "proof_mismatch");
    }

    #[test]
    fn invitation_revoked_serializes_reason() {
        let event = SetupInvitationRevokedEvent {
            reason: "cancelled".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["reason"], "cancelled");
    }

    #[test]
    fn invitation_issued_round_trips() {
        let event = SetupInvitationIssuedEvent {
            code: "WXYZ".to_string(),
            expires_at_ms: 42,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: SetupInvitationIssuedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
    }
}
