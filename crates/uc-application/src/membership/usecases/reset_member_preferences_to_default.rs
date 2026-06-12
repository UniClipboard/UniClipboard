use std::sync::Arc;

use uc_core::{DeviceId, MemberRepositoryPort, MemberSyncPreferences, SpaceMember};

use crate::membership::errors::MembershipApplicationError;

/// Input for resetting a member's sync preferences back to defaults.
#[derive(Debug, Clone)]
pub struct ResetMemberPreferencesToDefault {
    pub device_id: DeviceId,
}

/// 把本机对某成员的同步偏好重置为默认值。
///
/// 对应"设备详情页 → 恢复默认"按钮的语义。其它字段（device_name、
/// identity_fingerprint、joined_at）保持不变。
pub struct ResetMemberPreferencesToDefaultUseCase<R> {
    repository: Arc<R>,
}

impl<R> ResetMemberPreferencesToDefaultUseCase<R>
where
    R: MemberRepositoryPort,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        input: ResetMemberPreferencesToDefault,
    ) -> Result<SpaceMember, MembershipApplicationError> {
        let existing = self
            .repository
            .get(&input.device_id)
            .await?
            .ok_or_else(|| MembershipApplicationError::NotFound(input.device_id.clone()))?;

        let updated = SpaceMember {
            sync_preferences: MemberSyncPreferences::default(),
            ..existing
        };

        self.repository.save(&updated).await?;
        Ok(updated)
    }
}
