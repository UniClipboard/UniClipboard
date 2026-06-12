use std::sync::Arc;

use uc_core::{DeviceId, MemberRepositoryPort};

use crate::membership::errors::MembershipApplicationError;

/// Input for revoking a member.
#[derive(Debug, Clone)]
pub struct RevokeMember {
    pub device_id: DeviceId,
}

/// 撤销成员：直接从本机成员仓库中移除该设备。
///
/// 本地自治模型下不会广播通知对端。对端若仍尝试同步，
/// 需要由接收路径自行检查发送方是否仍在本机成员列表中。
pub struct RevokeMemberUseCase<R: ?Sized> {
    repository: Arc<R>,
}

impl<R> RevokeMemberUseCase<R>
where
    R: MemberRepositoryPort + ?Sized,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn execute(&self, input: RevokeMember) -> Result<(), MembershipApplicationError> {
        if !self.repository.remove(&input.device_id).await? {
            return Err(MembershipApplicationError::NotFound(input.device_id));
        }
        Ok(())
    }
}
