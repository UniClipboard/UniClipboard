//! DTOs for per-member sync preferences (phase 4b PR-2).
//!
//! 语义：映射 `SpaceMember.sync_preferences`（双向 `send_enabled` /
//! `receive_enabled` + 双套 `content_types`）。复用 `dto::settings` 下的
//! `ContentTypesDto` / `ContentTypesPatchDto`，两套字段形状一致。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use uc_core::membership::MemberSyncPreferences;

use super::settings::{ContentTypesDto, ContentTypesPatchDto};

/// Sync preferences recorded for a space member.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberSyncPreferencesDto {
    pub send_enabled: bool,
    pub receive_enabled: bool,
    pub send_content_types: ContentTypesDto,
    pub receive_content_types: ContentTypesDto,
}

/// Partial sync preferences for PATCH /member/:device_id/sync-preferences.
///
/// 服务器侧 `get → merge → save` 后持久化；未提供的字段保留当前值。
/// 重置到默认值的调用方应显式传入所有字段的默认值（`MemberSyncPreferences::default()`）。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberSyncPreferencesPatchDto {
    pub send_enabled: Option<bool>,
    pub receive_enabled: Option<bool>,
    pub send_content_types: Option<ContentTypesPatchDto>,
    pub receive_content_types: Option<ContentTypesPatchDto>,
}

/// Folded payload for `PATCH /member/:device_id/sync-preferences` (ADR-008 §0.1).
///
/// The current handler returns `success` as a top-level sibling of the
/// `{data,ts}` envelope. This DTO folds it INTO the payload so the endpoint can
/// return `ApiEnvelope<MemberSyncResultDto>` with no bespoke wrapper. P1 only
/// defines the type; the handler is rewired in P2.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberSyncResultDto {
    pub success: bool,
}

impl From<MemberSyncPreferences> for MemberSyncPreferencesDto {
    fn from(value: MemberSyncPreferences) -> Self {
        Self {
            send_enabled: value.send_enabled,
            receive_enabled: value.receive_enabled,
            send_content_types: value.send_content_types.into(),
            receive_content_types: value.receive_content_types.into(),
        }
    }
}
