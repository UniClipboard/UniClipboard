//! Multi-space profile HTTP DTOs for `/v2/spaces`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// User-observable state of one supervised space runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "state"
)]
pub enum SpaceRuntimeStateDto {
    Stopped,
    Starting,
    Running,
    Locked,
    Failed {
        #[schema(rename = "errorCategory")]
        error_category: String,
    },
}

/// Summary returned for one profile by `GET /v2/spaces` and space mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpaceProfileSummaryDto {
    pub profile_id: String,
    pub space_id: Option<String>,
    pub device_name: Option<String>,
    pub runtime_state: SpaceRuntimeStateDto,
    pub is_active_send: bool,
}

/// Request body for `POST /v2/spaces`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSpaceProfileRequestDto {
    pub passphrase: String,
    pub passphrase_confirm: String,
    pub device_name: Option<String>,
}

/// Request body for `POST /v2/spaces/join`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JoinSpaceProfileRequestDto {
    pub code: String,
    pub passphrase: String,
    pub device_name: Option<String>,
}

/// Request body for `PUT /v2/spaces/active-send`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
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
    fn profile_summary_serializes_the_user_observable_runtime_state() {
        let summary = SpaceProfileSummaryDto {
            profile_id: "profile-1".to_string(),
            space_id: Some("space-1".to_string()),
            device_name: Some("Office PC".to_string()),
            runtime_state: SpaceRuntimeStateDto::Failed {
                error_category: "engine_start_failed".to_string(),
            },
            is_active_send: true,
        };

        let wire = serde_json::to_value(summary).expect("serialize");
        assert_eq!(
            wire,
            serde_json::json!({
                "profileId": "profile-1",
                "spaceId": "space-1",
                "deviceName": "Office PC",
                "runtimeState": {
                    "state": "failed",
                    "errorCategory": "engine_start_failed"
                },
                "isActiveSend": true
            })
        );
        assert!(wire.get("profile_id").is_none());
        assert!(wire["runtimeState"].get("error_category").is_none());
    }

    #[test]
    fn create_and_join_requests_use_v2_setup_camel_case_names() {
        let create = CreateSpaceProfileRequestDto {
            passphrase: "hunter22hunter22".to_string(),
            passphrase_confirm: "hunter22hunter22".to_string(),
            device_name: Some("Office PC".to_string()),
        };
        let join = JoinSpaceProfileRequestDto {
            code: "ABCD-1234".to_string(),
            passphrase: "hunter22hunter22".to_string(),
            device_name: Some("Laptop".to_string()),
        };

        assert_eq!(
            serde_json::to_value(create).expect("serialize"),
            serde_json::json!({
                "passphrase": "hunter22hunter22",
                "passphraseConfirm": "hunter22hunter22",
                "deviceName": "Office PC"
            })
        );
        let join_wire = serde_json::to_value(join).expect("serialize");
        assert_eq!(
            join_wire,
            serde_json::json!({
                "code": "ABCD-1234",
                "passphrase": "hunter22hunter22",
                "deviceName": "Laptop"
            })
        );
        assert!(join_wire.get("device_name").is_none());
    }

    #[test]
    fn active_send_request_serializes_only_the_selected_profile_id() {
        let request = SetActiveSendSpaceRequestDto {
            profile_id: "profile-2".to_string(),
        };

        let wire = serde_json::to_value(request).expect("serialize");
        assert_eq!(wire, serde_json::json!({ "profileId": "profile-2" }));
        assert!(wire.get("profile_id").is_none());
    }
}
