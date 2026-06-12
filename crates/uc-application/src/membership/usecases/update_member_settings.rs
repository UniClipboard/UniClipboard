use std::sync::Arc;

use uc_core::{DeviceId, MemberRepositoryPort, MemberSyncPreferences, SpaceMember};

use crate::membership::errors::MembershipApplicationError;

/// Input for fully overwriting a member's sync preferences.
///
/// 全量覆盖语义：传入的 `sync_preferences` 将整体替换原有值。
/// UI 层如果只想改局部字段，应在构造 command 时先把未改动字段从
/// 当前值里填好，再交给本 use case。
#[derive(Debug, Clone)]
pub struct UpdateMemberSettings {
    pub device_id: DeviceId,
    pub sync_preferences: MemberSyncPreferences,
}

/// 更新本机对某成员的同步偏好（全量覆盖）。
pub struct UpdateMemberSettingsUseCase<R: ?Sized> {
    repository: Arc<R>,
}

impl<R> UpdateMemberSettingsUseCase<R>
where
    R: MemberRepositoryPort + ?Sized,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        input: UpdateMemberSettings,
    ) -> Result<SpaceMember, MembershipApplicationError> {
        let existing = self
            .repository
            .get(&input.device_id)
            .await?
            .ok_or_else(|| MembershipApplicationError::NotFound(input.device_id.clone()))?;

        let updated = SpaceMember {
            sync_preferences: input.sync_preferences,
            ..existing
        };

        self.repository.save(&updated).await?;
        Ok(updated)
    }
}
