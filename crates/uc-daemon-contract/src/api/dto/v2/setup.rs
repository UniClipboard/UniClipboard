//! Stateless v2 setup pairing HTTP DTOs (Slice4 Phase 3 T3.2).
//!
//! Mirrors the new `SpaceSetupFacade` surface and lives under the
//! `/v2/setup/*` route namespace. The legacy `dto::setup` module is
//! deleted by T3.4 in one shot; the v2 directory survives.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ---------------------------------------------------------------------------
// POST /v2/setup/initialize
// ---------------------------------------------------------------------------

/// Request body for `POST /v2/setup/initialize`. Maps to
/// `SpaceSetupFacade::initialize_space(InitializeSpaceCommand)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitializeSpaceRequest {
    pub passphrase: String,
    pub passphrase_confirm: String,
    pub device_name: Option<String>,
}

/// Response body for `POST /v2/setup/initialize`. Mirrors
/// `InitializeSpaceResult` flattened to wire-friendly strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitializeSpaceResponse {
    pub space_id: String,
    pub self_device_id: String,
    pub fingerprint: String,
}

// ---------------------------------------------------------------------------
// POST /v2/setup/issue-invitation
// ---------------------------------------------------------------------------

/// Response body for `POST /v2/setup/issue-invitation`. Mirrors
/// `IssuePairingInvitationResult` with an epoch-millis expiry to keep
/// the wire form free of timezone parsing on the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IssueInvitationResponse {
    pub code: String,
    pub expires_at_ms: i64,
}

// ---------------------------------------------------------------------------
// POST /v2/setup/redeem
// ---------------------------------------------------------------------------

/// Request body for `POST /v2/setup/redeem`. Maps to
/// `SpaceSetupFacade::redeem_pairing_invitation(RedeemPairingInvitationCommand)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RedeemRequest {
    pub code: String,
    pub passphrase: String,
}

/// Stable outcome of a durable space admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "status"
)]
pub enum JoinSpaceResponse {
    Active {
        #[schema(rename = "joinId")]
        join_id: String,
        #[schema(rename = "joinedSpace")]
        joined_space: JoinedSpaceResponse,
    },
    Pending {
        #[schema(rename = "joinId")]
        join_id: String,
        #[schema(rename = "targetSpaceId")]
        target_space_id: Option<String>,
        #[schema(rename = "sponsorDeviceId")]
        sponsor_device_id: Option<String>,
        #[schema(rename = "sponsorIdentityFingerprint")]
        sponsor_identity_fingerprint: Option<String>,
        #[schema(rename = "cancelRequested")]
        cancel_requested: bool,
    },
    Rejected {
        #[schema(rename = "joinId")]
        join_id: String,
        reason: JoinSpaceRejectionReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JoinedSpaceResponse {
    pub sponsor_device_id: String,
    pub sponsor_identity_fingerprint: String,
    pub space_id: String,
    pub self_device_id: String,
    pub self_identity_fingerprint: String,
    pub migrated_records: Option<u64>,
    pub preserved_unreadable_records: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JoinSpaceRejectionReason {
    InvitationUnavailable,
    AuthenticationRejected,
    IdentityConflict,
    BaseHistoryChanged,
    JoinerHistoryAhead,
    HistoryConflict,
    PeerUpgradeRequired,
    Cancelled,
    RemovedBeforeActivation,
}

// ---------------------------------------------------------------------------
// GET /v2/setup/state
// ---------------------------------------------------------------------------

/// Response body for `GET /v2/setup/state`. Mirrors `SetupStateView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupStateResponse {
    pub has_completed: bool,
    pub current_invitation: Option<CurrentInvitation>,
    pub device_name: Option<String>,
    pub re_pairing_required: bool,
}

/// Companion to [`SetupStateResponse::current_invitation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrentInvitation {
    pub code: String,
    pub expires_at_ms: i64,
}

// POST /v2/setup/cancel and POST /v2/setup/reset return HTTP 204 No
// Content with no body — no response DTO needed. 409 Conflict on
// cancel-when-empty surfaces through the daemon's standard ApiError.

// ---------------------------------------------------------------------------
// POST /v2/setup/switch-space
// ---------------------------------------------------------------------------

/// Request body for `POST /v2/setup/switch-space`. Maps to
/// `SpaceSetupFacade::switch_space(SwitchSpaceInput)`. Pre-conditions
/// (setup completed + session unlocked + no pending migration) are
/// checked inside the facade and surface as 409 / 423 / 423 respectively.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SwitchSpaceRequest {
    pub code: String,
    pub new_passphrase: String,
    #[serde(default)]
    pub preserve_unreadable_history: bool,
}

/// Request body for `POST /v2/setup/cancel-join`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelJoinSpaceRequest {
    pub join_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_request_round_trip_camel_case() {
        let req = InitializeSpaceRequest {
            passphrase: "hunter22hunter22".to_string(),
            passphrase_confirm: "hunter22hunter22".to_string(),
            device_name: Some("MacBook".to_string()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["passphrase"], "hunter22hunter22");
        assert_eq!(json["passphraseConfirm"], "hunter22hunter22");
        assert_eq!(json["deviceName"], "MacBook");
        assert!(json.get("passphrase_confirm").is_none());
        let decoded: InitializeSpaceRequest = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn initialize_response_serializes_camel_case() {
        let resp = InitializeSpaceResponse {
            space_id: "space-1".to_string(),
            self_device_id: "device-1".to_string(),
            fingerprint: "ABCDEFGHIJKLMNOP".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["spaceId"], "space-1");
        assert_eq!(json["selfDeviceId"], "device-1");
        assert_eq!(json["fingerprint"], "ABCDEFGHIJKLMNOP");
    }

    #[test]
    fn issue_invitation_response_uses_epoch_millis() {
        let resp = IssueInvitationResponse {
            code: "ABCD-1234".to_string(),
            expires_at_ms: 1_745_577_600_000,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["code"], "ABCD-1234");
        assert_eq!(json["expiresAtMs"], 1_745_577_600_000_i64);
    }

    #[test]
    fn redeem_request_round_trip() {
        let req = RedeemRequest {
            code: "WXYZ-5678".to_string(),
            passphrase: "hunter22hunter22".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: RedeemRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn join_space_active_response_carries_both_sides() {
        let resp = JoinSpaceResponse::Active {
            join_id: "join-1".to_string(),
            joined_space: JoinedSpaceResponse {
                sponsor_device_id: "sponsor-1".to_string(),
                sponsor_identity_fingerprint: "FPSPONSOR".to_string(),
                space_id: "space-1".to_string(),
                self_device_id: "joiner-2".to_string(),
                self_identity_fingerprint: "FPJOINER".to_string(),
                migrated_records: Some(7),
                preserved_unreadable_records: Some(2),
            },
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "active");
        assert_eq!(json["joinId"], "join-1");
        assert_eq!(json["joinedSpace"]["sponsorDeviceId"], "sponsor-1");
        assert_eq!(json["joinedSpace"]["spaceId"], "space-1");
    }

    #[test]
    fn state_response_with_pending_invitation() {
        let resp = SetupStateResponse {
            has_completed: true,
            current_invitation: Some(CurrentInvitation {
                code: "ABCD-1234".to_string(),
                expires_at_ms: 1_745_577_600_000,
            }),
            device_name: Some("MacBook".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["hasCompleted"], true);
        assert_eq!(json["currentInvitation"]["code"], "ABCD-1234");
        assert_eq!(
            json["currentInvitation"]["expiresAtMs"],
            1_745_577_600_000_i64
        );
        assert_eq!(json["deviceName"], "MacBook");
    }

    #[test]
    fn switch_space_request_round_trip_camel_case() {
        let req = SwitchSpaceRequest {
            code: "ABCD-1234".to_string(),
            new_passphrase: "newpass22newpass".to_string(),
            preserve_unreadable_history: false,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["code"], "ABCD-1234");
        assert_eq!(json["newPassphrase"], "newpass22newpass");
        assert_eq!(json["preserveUnreadableHistory"], false);
        assert!(json.get("new_passphrase").is_none());
        let decoded: SwitchSpaceRequest = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, req);

        let legacy: SwitchSpaceRequest = serde_json::from_value(serde_json::json!({
            "code": "ABCD-1234",
            "newPassphrase": "newpass22newpass",
        }))
        .unwrap();
        assert!(!legacy.preserve_unreadable_history);
    }

    #[test]
    fn state_response_fresh_install_serializes_nulls() {
        let resp = SetupStateResponse {
            has_completed: false,
            current_invitation: None,
            device_name: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["hasCompleted"], false);
        assert!(json["currentInvitation"].is_null());
        assert!(json["deviceName"].is_null());
    }

    #[test]
    fn join_space_pending_response_preserves_the_join_id_and_camel_case_fields() {
        let response = JoinSpaceResponse::Pending {
            join_id: "join-1".to_string(),
            target_space_id: Some("space-1".to_string()),
            sponsor_device_id: Some("sponsor-1".to_string()),
            sponsor_identity_fingerprint: Some("fingerprint-1".to_string()),
            cancel_requested: false,
        };

        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["status"], "pending");
        assert_eq!(json["joinId"], "join-1");
        assert_eq!(json["targetSpaceId"], "space-1");
        assert_eq!(json["sponsorDeviceId"], "sponsor-1");
        assert_eq!(json["sponsorIdentityFingerprint"], "fingerprint-1");
        assert_eq!(json["cancelRequested"], false);
        assert!(json.get("join_id").is_none());
    }
}
