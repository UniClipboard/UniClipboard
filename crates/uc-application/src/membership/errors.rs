use thiserror::Error;

use uc_core::{DeviceId, MembershipError};

/// Application-layer errors for membership use cases.
///
/// 成员管理应用层错误 —— 表达"这次应用动作为什么不能继续"，
/// 不承担底层存储实现的细节语义。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MembershipApplicationError {
    /// 目标设备已经作为成员被接纳过，重复 admit 被拒绝。
    #[error("member `{0}` has already been admitted")]
    AlreadyAdmitted(DeviceId),

    /// 目标成员不存在于本地成员列表。
    #[error("member `{0}` not found")]
    NotFound(DeviceId),

    /// 跨越 repository port 边界的基础设施失败。
    #[error("membership repository failure: {0}")]
    Repository(String),
}

impl From<MembershipError> for MembershipApplicationError {
    fn from(err: MembershipError) -> Self {
        match err {
            MembershipError::AlreadyAdmitted(id) => Self::AlreadyAdmitted(id),
            MembershipError::NotFound(id) => Self::NotFound(id),
            MembershipError::Repository(msg) => Self::Repository(msg),
        }
    }
}
