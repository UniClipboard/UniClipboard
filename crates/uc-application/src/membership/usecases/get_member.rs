use std::sync::Arc;

use uc_core::{DeviceId, MemberRepositoryPort, SpaceMember};

use crate::membership::errors::MembershipApplicationError;

/// Input for looking up a single member by device id.
#[derive(Debug, Clone)]
pub struct GetMember {
    pub device_id: DeviceId,
}

/// 按 `DeviceId` 查询单个成员；不存在时返回 `NotFound`。
///
/// 面向 UI 的"设备详情"视图使用。若调用方希望"存在性可选"，
/// 后续可引入一个独立的 `FindMember` 用例，直接返回 `Option`。
pub struct GetMemberUseCase<R: ?Sized> {
    repository: Arc<R>,
}

impl<R> GetMemberUseCase<R>
where
    R: MemberRepositoryPort + ?Sized,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        input: GetMember,
    ) -> Result<SpaceMember, MembershipApplicationError> {
        self.repository
            .get(&input.device_id)
            .await?
            .ok_or(MembershipApplicationError::NotFound(input.device_id))
    }
}
