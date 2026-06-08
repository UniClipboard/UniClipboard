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

/// Response body for `POST /v2/setup/redeem`. Mirrors
/// `RedeemPairingInvitationResult` flattened to wire-friendly strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RedeemResponse {
    pub sponsor_device_id: String,
    pub sponsor_identity_fingerprint: String,
    pub space_id: String,
    pub self_device_id: String,
    pub self_identity_fingerprint: String,
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
}

/// Response body for `POST /v2/setup/switch-space`. Mirrors
/// `SwitchSpaceResult` flattened to wire-friendly strings, plus
/// `migrated_records` so the UI can show "迁移了 N 条历史".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SwitchSpaceResponse {
    pub sponsor_device_id: String,
    pub sponsor_identity_fingerprint: String,
    pub space_id: String,
    pub self_device_id: String,
    pub self_identity_fingerprint: String,
    pub migrated_records: u64,
}

// ---------------------------------------------------------------------------
// GET /v2/setup/migration-progress
// ---------------------------------------------------------------------------

/// Coarse-grained migration phase exposed to the UI. The internal
/// `MigrationPhase` carries `run_id` / `target_space_id`; those are
/// implementation detail and not surfaced over the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhaseDto {
    Prepared,
    HandshakeDone,
    Swapped,
}

/// Response body for `GET /v2/setup/migration-progress`. Mirrors
/// `MigrationProgress`. `phase = null` means no migration is in
/// flight (idle / completed); polling clients should stop their loop
/// when they observe this transition while `backup_record_count == 0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MigrationProgressResponse {
    pub phase: Option<MigrationPhaseDto>,
    pub backup_record_count: u64,
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
    fn redeem_response_carries_both_sides() {
        let resp = RedeemResponse {
            sponsor_device_id: "sponsor-1".to_string(),
            sponsor_identity_fingerprint: "FPSPONSOR".to_string(),
            space_id: "space-1".to_string(),
            self_device_id: "joiner-2".to_string(),
            self_identity_fingerprint: "FPJOINER".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["sponsorDeviceId"], "sponsor-1");
        assert_eq!(json["sponsorIdentityFingerprint"], "FPSPONSOR");
        assert_eq!(json["spaceId"], "space-1");
        assert_eq!(json["selfDeviceId"], "joiner-2");
        assert_eq!(json["selfIdentityFingerprint"], "FPJOINER");
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
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["code"], "ABCD-1234");
        assert_eq!(json["newPassphrase"], "newpass22newpass");
        assert!(json.get("new_passphrase").is_none());
        let decoded: SwitchSpaceRequest = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn switch_space_response_carries_migrated_records() {
        let resp = SwitchSpaceResponse {
            sponsor_device_id: "sponsor-1".to_string(),
            sponsor_identity_fingerprint: "FPSPONSOR".to_string(),
            space_id: "space-1".to_string(),
            self_device_id: "joiner-2".to_string(),
            self_identity_fingerprint: "FPJOINER".to_string(),
            migrated_records: 7,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["sponsorDeviceId"], "sponsor-1");
        assert_eq!(json["spaceId"], "space-1");
        assert_eq!(json["selfDeviceId"], "joiner-2");
        assert_eq!(json["migratedRecords"], 7);
    }

    #[test]
    fn migration_progress_idle_serializes_phase_null() {
        let resp = MigrationProgressResponse {
            phase: None,
            backup_record_count: 0,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["phase"].is_null());
        assert_eq!(json["backupRecordCount"], 0);
    }

    #[test]
    fn migration_progress_in_flight_serializes_snake_case_phase() {
        let resp = MigrationProgressResponse {
            phase: Some(MigrationPhaseDto::HandshakeDone),
            backup_record_count: 42,
        };
        let json = serde_json::to_value(&resp).unwrap();
        // The phase enum is the wire contract — UI clients pattern-match
        // on these strings, so the snake_case rendering is ironclad.
        assert_eq!(json["phase"], "handshake_done");
        assert_eq!(json["backupRecordCount"], 42);
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
}
