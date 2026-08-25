//! Multi-space profile HTTP DTOs for `/v2/spaces`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// User-observable state of one supervised space runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "state",
    deny_unknown_fields
)]
pub enum SpaceRuntimeStateDto {
    Stopped,
    Starting,
    Running,
    Locked,
    Failed,
}

/// User-observable state of inbound synchronization for one space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "state",
    deny_unknown_fields
)]
pub enum SpaceIncomingSyncStateDto {
    Enabled,
    Receiving,
    Degraded,
    Disabled,
}

/// Last non-sensitive fault retained independently of runtime lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpaceFaultDto {
    pub category: String,
    pub message_code: Option<String>,
}

/// Summary returned for one profile by `GET /v2/spaces` and space mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpaceProfileSummaryDto {
    pub profile_id: String,
    pub space_id: Option<String>,
    pub display_name: Option<String>,
    pub device_name: Option<String>,
    pub runtime_state: SpaceRuntimeStateDto,
    pub incoming_sync_state: SpaceIncomingSyncStateDto,
    pub last_fault: Option<SpaceFaultDto>,
    pub is_active_send: bool,
}

/// Request body for `POST /v2/spaces`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSpaceProfileRequestDto {
    pub passphrase: String,
    pub passphrase_confirm: String,
    pub device_name: Option<String>,
}

/// Request body for `POST /v2/spaces/join`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JoinSpaceProfileRequestDto {
    pub code: String,
    pub passphrase: String,
    pub device_name: Option<String>,
}

/// Request body for `PUT /v2/spaces/active-send`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetActiveSendSpaceRequestDto {
    pub profile_id: String,
}

// DELETE /v2/spaces/{profileId} carries the profile ID in the path. The
// handler enforces that the explicitly named runtime has stopped, so no body
// DTO is needed.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_states_have_exact_snake_case_discriminators_and_round_trip() {
        let cases = [
            (
                SpaceRuntimeStateDto::Stopped,
                serde_json::json!({ "state": "stopped" }),
            ),
            (
                SpaceRuntimeStateDto::Starting,
                serde_json::json!({ "state": "starting" }),
            ),
            (
                SpaceRuntimeStateDto::Running,
                serde_json::json!({ "state": "running" }),
            ),
            (
                SpaceRuntimeStateDto::Locked,
                serde_json::json!({ "state": "locked" }),
            ),
            (
                SpaceRuntimeStateDto::Failed,
                serde_json::json!({ "state": "failed" }),
            ),
        ];

        for (state, expected) in cases {
            let wire = serde_json::to_value(&state).expect("serialize runtime state");
            assert_eq!(wire, expected);
            let decoded: SpaceRuntimeStateDto =
                serde_json::from_value(expected).expect("deserialize runtime state");
            assert_eq!(decoded, state);
        }
    }

    #[test]
    fn incoming_sync_states_have_exact_snake_case_discriminators_and_round_trip() {
        let cases = [
            (
                SpaceIncomingSyncStateDto::Enabled,
                serde_json::json!({ "state": "enabled" }),
            ),
            (
                SpaceIncomingSyncStateDto::Receiving,
                serde_json::json!({ "state": "receiving" }),
            ),
            (
                SpaceIncomingSyncStateDto::Degraded,
                serde_json::json!({ "state": "degraded" }),
            ),
            (
                SpaceIncomingSyncStateDto::Disabled,
                serde_json::json!({ "state": "disabled" }),
            ),
        ];

        for (state, expected) in cases {
            let wire = serde_json::to_value(&state).expect("serialize incoming sync state");
            assert_eq!(wire, expected);
            let decoded: SpaceIncomingSyncStateDto =
                serde_json::from_value(expected).expect("deserialize incoming sync state");
            assert_eq!(decoded, state);
        }
    }

    #[test]
    fn profile_summary_some_values_have_exact_camel_case_wire_shape_and_round_trip() {
        let summary = SpaceProfileSummaryDto {
            profile_id: "profile-1".to_string(),
            space_id: Some("space-1".to_string()),
            display_name: Some("Work Space".to_string()),
            device_name: Some("Office PC".to_string()),
            runtime_state: SpaceRuntimeStateDto::Locked,
            incoming_sync_state: SpaceIncomingSyncStateDto::Degraded,
            last_fault: Some(SpaceFaultDto {
                category: "network".to_string(),
                message_code: Some("relay_unreachable".to_string()),
            }),
            is_active_send: true,
        };

        let wire = serde_json::to_value(&summary).expect("serialize summary");
        assert_eq!(
            wire,
            serde_json::json!({
                "profileId": "profile-1",
                "spaceId": "space-1",
                "displayName": "Work Space",
                "deviceName": "Office PC",
                "runtimeState": { "state": "locked" },
                "incomingSyncState": { "state": "degraded" },
                "lastFault": {
                    "category": "network",
                    "messageCode": "relay_unreachable"
                },
                "isActiveSend": true
            })
        );
        assert!(wire.get("profile_id").is_none());
        assert!(wire.get("incoming_sync_state").is_none());
        assert!(wire["lastFault"].get("message_code").is_none());

        let decoded: SpaceProfileSummaryDto =
            serde_json::from_value(wire).expect("deserialize summary");
        assert_eq!(decoded, summary);
    }

    #[test]
    fn profile_summary_none_values_have_exact_wire_shape_and_round_trip() {
        let summary = SpaceProfileSummaryDto {
            profile_id: "profile-2".to_string(),
            space_id: None,
            display_name: None,
            device_name: None,
            runtime_state: SpaceRuntimeStateDto::Stopped,
            incoming_sync_state: SpaceIncomingSyncStateDto::Disabled,
            last_fault: None,
            is_active_send: false,
        };

        let expected = serde_json::json!({
            "profileId": "profile-2",
            "spaceId": null,
            "displayName": null,
            "deviceName": null,
            "runtimeState": { "state": "stopped" },
            "incomingSyncState": { "state": "disabled" },
            "lastFault": null,
            "isActiveSend": false
        });
        let wire = serde_json::to_value(&summary).expect("serialize summary");
        assert_eq!(wire, expected);
        let decoded: SpaceProfileSummaryDto =
            serde_json::from_value(expected).expect("deserialize summary");
        assert_eq!(decoded, summary);
    }

    #[test]
    fn profile_summary_missing_optional_fields_deserialize_as_none() {
        let decoded: SpaceProfileSummaryDto = serde_json::from_value(serde_json::json!({
            "profileId": "profile-3",
            "runtimeState": { "state": "running" },
            "incomingSyncState": { "state": "receiving" },
            "isActiveSend": false
        }))
        .expect("deserialize summary without optional fields");

        assert_eq!(decoded.space_id, None);
        assert_eq!(decoded.display_name, None);
        assert_eq!(decoded.device_name, None);
        assert_eq!(decoded.last_fault, None);
    }

    #[test]
    fn profile_summary_missing_is_active_send_is_rejected_fail_closed() {
        let result = serde_json::from_value::<SpaceProfileSummaryDto>(serde_json::json!({
            "profileId": "profile-3",
            "runtimeState": { "state": "running" },
            "incomingSyncState": { "state": "enabled" }
        }));

        assert!(result.is_err());
    }

    #[test]
    fn create_request_has_exact_wire_shape_and_round_trip() {
        let create = CreateSpaceProfileRequestDto {
            passphrase: "hunter22hunter22".to_string(),
            passphrase_confirm: "hunter22hunter22".to_string(),
            device_name: Some("Office PC".to_string()),
        };

        let wire = serde_json::to_value(&create).expect("serialize create request");
        assert_eq!(
            wire,
            serde_json::json!({
                "passphrase": "hunter22hunter22",
                "passphraseConfirm": "hunter22hunter22",
                "deviceName": "Office PC"
            })
        );
        let decoded: CreateSpaceProfileRequestDto =
            serde_json::from_value(wire).expect("deserialize create request");
        assert_eq!(decoded, create);
    }

    #[test]
    fn join_request_has_exact_wire_shape_and_round_trip() {
        let join = JoinSpaceProfileRequestDto {
            code: "ABCD-1234".to_string(),
            passphrase: "hunter22hunter22".to_string(),
            device_name: None,
        };

        let wire = serde_json::to_value(&join).expect("serialize join request");
        assert_eq!(
            wire,
            serde_json::json!({
                "code": "ABCD-1234",
                "passphrase": "hunter22hunter22",
                "deviceName": null
            })
        );
        let decoded: JoinSpaceProfileRequestDto =
            serde_json::from_value(wire).expect("deserialize join request");
        assert_eq!(decoded, join);
    }

    #[test]
    fn active_send_request_has_exact_wire_shape_and_round_trip() {
        let request = SetActiveSendSpaceRequestDto {
            profile_id: "profile-2".to_string(),
        };

        let wire = serde_json::to_value(&request).expect("serialize active-send request");
        assert_eq!(wire, serde_json::json!({ "profileId": "profile-2" }));
        assert!(wire.get("profile_id").is_none());
        let decoded: SetActiveSendSpaceRequestDto =
            serde_json::from_value(wire).expect("deserialize active-send request");
        assert_eq!(decoded, request);
    }

    #[test]
    fn missing_optional_request_fields_deserialize_as_none() {
        let create: CreateSpaceProfileRequestDto = serde_json::from_value(serde_json::json!({
            "passphrase": "hunter22hunter22",
            "passphraseConfirm": "hunter22hunter22"
        }))
        .expect("deserialize create request without device name");
        let join: JoinSpaceProfileRequestDto = serde_json::from_value(serde_json::json!({
            "code": "ABCD-1234",
            "passphrase": "hunter22hunter22"
        }))
        .expect("deserialize join request without device name");

        assert_eq!(create.device_name, None);
        assert_eq!(join.device_name, None);
    }

    #[test]
    fn legacy_snake_case_fields_are_rejected() {
        let summary = serde_json::from_value::<SpaceProfileSummaryDto>(serde_json::json!({
            "profile_id": "profile-1",
            "runtime_state": { "state": "running" },
            "incoming_sync_state": { "state": "receiving" },
            "is_active_send": true
        }));
        let create = serde_json::from_value::<CreateSpaceProfileRequestDto>(serde_json::json!({
            "passphrase": "hunter22hunter22",
            "passphrase_confirm": "hunter22hunter22"
        }));
        let join = serde_json::from_value::<JoinSpaceProfileRequestDto>(serde_json::json!({
            "code": "ABCD-1234",
            "passphrase": "hunter22hunter22",
            "device_name": "Laptop"
        }));
        let active_send =
            serde_json::from_value::<SetActiveSendSpaceRequestDto>(serde_json::json!({
                "profile_id": "profile-2"
            }));

        assert!(summary.is_err());
        assert!(create.is_err());
        assert!(join.is_err());
        assert!(active_send.is_err());
    }
}
