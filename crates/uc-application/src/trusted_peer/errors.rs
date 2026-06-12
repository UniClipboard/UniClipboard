use thiserror::Error;

use uc_core::{DeviceId, TrustedPeerError};

/// Application-layer errors for the trusted-peer domain.
///
/// 表达"这次应用动作为什么不能继续"，不承担底层存储实现的细节语义。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TrustedPeerApplicationError {
    /// 目标设备已经作为 trusted peer 登记过，重复建立信任被拒绝。
    #[error("peer `{0}` is already trusted")]
    AlreadyTrusted(DeviceId),

    /// 目标 peer 不存在于本机 trusted peer 列表。
    #[error("trusted peer `{0}` not found")]
    NotFound(DeviceId),

    /// 状态机当前状态下不接受该事件。
    #[error("illegal trust-state transition: {0}")]
    IllegalTransition(String),

    /// 跨越 repository port 边界的基础设施失败。
    #[error("trusted-peer repository failure: {0}")]
    Repository(String),
}

impl From<TrustedPeerError> for TrustedPeerApplicationError {
    fn from(err: TrustedPeerError) -> Self {
        match err {
            TrustedPeerError::AlreadyTrusted(id) => Self::AlreadyTrusted(id),
            TrustedPeerError::NotFound(id) => Self::NotFound(id),
            TrustedPeerError::Repository(msg) => Self::Repository(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_core_error_preserves_variant_and_payload() {
        let dev = DeviceId::new("peer-x");

        assert_eq!(
            TrustedPeerApplicationError::from(TrustedPeerError::AlreadyTrusted(dev.clone())),
            TrustedPeerApplicationError::AlreadyTrusted(dev.clone())
        );
        assert_eq!(
            TrustedPeerApplicationError::from(TrustedPeerError::NotFound(dev.clone())),
            TrustedPeerApplicationError::NotFound(dev)
        );
        assert_eq!(
            TrustedPeerApplicationError::from(TrustedPeerError::Repository("boom".into())),
            TrustedPeerApplicationError::Repository("boom".into())
        );
    }
}
