use std::sync::Arc;

use uc_core::{MemberRepositoryPort, SpaceMember};

use crate::membership::errors::MembershipApplicationError;

/// 列出本机空间内所有已接纳的成员。
///
/// 面向"设备管理页"等 UI 消费。单空间模型下不需要额外输入。
pub struct ListMembersUseCase<R: ?Sized> {
    repository: Arc<R>,
}

impl<R> ListMembersUseCase<R>
where
    R: MemberRepositoryPort + ?Sized,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn execute(&self) -> Result<Vec<SpaceMember>, MembershipApplicationError> {
        Ok(self.repository.list().await?)
    }
}
