//! `RevokeMobileDeviceUseCase` —— 把一台已登记的 iPhone Shortcut 设备从
//! 服务端记录中移除。
//!
//! 撤销后该设备的 (username, password) 立即失效（middleware 用
//! `find_by_username` 找不到对应记录就 401）。这里只删服务端记录, 不主动
//! 通知客户端 —— Shortcut 没有反向通道, iPhone 端在下一次发请求拿到 401
//! 时由用户自行在 iOS 上删除该 Shortcut。
//!
//! 该 use case 没有自己的"动作产物"，成功仅返回 `()`：调用方拿到 `Ok`
//! 即视为撤销已生效。失败语义集中在
//! [`RevokeMobileDeviceError`]：要么"设备本就不存在"（NotFound，调用方
//! 据此提示用户列表已过期），要么"持久化层失败"（PersistenceFailed，
//! 调用方应允许重试）。

use std::sync::Arc;

use tracing::instrument;

use uc_core::mobile_sync::{MobileDeviceError, MobileDeviceId};
use uc_core::ports::MobileDeviceRepositoryPort;

// ─── public-shaped (input / error) ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RevokeMobileDeviceInput {
    pub device_id: MobileDeviceId,
}

#[derive(Debug, thiserror::Error)]
pub enum RevokeMobileDeviceError {
    /// 仓储里没找到该 device_id —— UI 列表过期 / 已被另一处撤销。
    /// 调用方据此提示用户刷新列表，而不是当作真正的失败。
    #[error("device not found: {0}")]
    NotFound(String),

    /// 持久化失败 —— 包含底层文案以便日志排障，调用方应允许重试。
    #[error("device persistence failed: {0}")]
    PersistenceFailed(String),
}

// ─── use case ───────────────────────────────────────────────────────────

pub(crate) struct RevokeMobileDeviceUseCase {
    device_repo: Arc<dyn MobileDeviceRepositoryPort>,
}

impl RevokeMobileDeviceUseCase {
    pub(crate) fn new(device_repo: Arc<dyn MobileDeviceRepositoryPort>) -> Self {
        Self { device_repo }
    }

    #[instrument(skip(self, input), fields(device_id = %input.device_id))]
    pub(crate) async fn execute(
        &self,
        input: RevokeMobileDeviceInput,
    ) -> Result<(), RevokeMobileDeviceError> {
        let removed = self
            .device_repo
            .delete(&input.device_id)
            .await
            .map_err(translate_device_error)?;
        if !removed {
            return Err(RevokeMobileDeviceError::NotFound(
                input.device_id.into_string(),
            ));
        }
        Ok(())
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

fn translate_device_error(err: MobileDeviceError) -> RevokeMobileDeviceError {
    match err {
        MobileDeviceError::Storage(msg) => RevokeMobileDeviceError::PersistenceFailed(msg),
        // delete 路径理论上不会触发 AlreadyExists / UsernameCollision;
        // 走到这里说明 adapter 违约, 转为 PersistenceFailed 兜底。
        other => RevokeMobileDeviceError::PersistenceFailed(other.to_string()),
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use mockall::predicate::eq;

    use uc_core::mobile_sync::MobileDevice;

    mockall::mock! {
        DeviceRepo {}
        #[async_trait]
        impl MobileDeviceRepositoryPort for DeviceRepo {
            async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError>;
            async fn find_by_username(
                &self,
                username: &str,
            ) -> Result<Option<MobileDevice>, MobileDeviceError>;
            async fn find_by_device_id(
                &self,
                device_id: &MobileDeviceId,
            ) -> Result<Option<MobileDevice>, MobileDeviceError>;
            async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError>;
            async fn delete(&self, device_id: &MobileDeviceId) -> Result<bool, MobileDeviceError>;
            async fn record_activity(
                &self,
                device_id: &MobileDeviceId,
                last_seen_at_ms: i64,
                last_seen_ip: Option<String>,
                reported_name: Option<String>,
                reported_os: Option<String>,
            ) -> Result<(), MobileDeviceError>;
            async fn update_password_hash(
                &self,
                device_id: &MobileDeviceId,
                new_password_hash: String,
            ) -> Result<bool, MobileDeviceError>;
        }
    }

    #[tokio::test]
    async fn deletes_existing_device() {
        let target = MobileDeviceId::new("did_x");
        let mut repo = MockDeviceRepo::new();
        repo.expect_delete()
            .with(eq(target.clone()))
            .times(1)
            .returning(|_| Ok(true));

        let uc = RevokeMobileDeviceUseCase::new(Arc::new(repo));
        uc.execute(RevokeMobileDeviceInput { device_id: target })
            .await
            .expect("should succeed");
    }

    #[tokio::test]
    async fn returns_not_found_when_missing() {
        let mut repo = MockDeviceRepo::new();
        repo.expect_delete().returning(|_| Ok(false));

        let uc = RevokeMobileDeviceUseCase::new(Arc::new(repo));
        let err = uc
            .execute(RevokeMobileDeviceInput {
                device_id: MobileDeviceId::new("did_missing"),
            })
            .await
            .unwrap_err();
        assert!(
            matches!(err, RevokeMobileDeviceError::NotFound(ref s) if s == "did_missing"),
            "expected NotFound(did_missing), got {err:?}"
        );
    }

    #[tokio::test]
    async fn translates_storage_error() {
        let mut repo = MockDeviceRepo::new();
        repo.expect_delete()
            .returning(|_| Err(MobileDeviceError::Storage("disk gone".into())));

        let uc = RevokeMobileDeviceUseCase::new(Arc::new(repo));
        let err = uc
            .execute(RevokeMobileDeviceInput {
                device_id: MobileDeviceId::new("did_x"),
            })
            .await
            .unwrap_err();
        assert!(
            matches!(err, RevokeMobileDeviceError::PersistenceFailed(ref s) if s.contains("disk gone")),
            "expected PersistenceFailed(disk gone), got {err:?}"
        );
    }
}
