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

    use std::sync::Mutex;

    use async_trait::async_trait;

    use uc_core::mobile_sync::MobileDevice;

    /// 极简内存 repo：只承担 `delete` 路径所需的语义；其它方法 panic
    /// 以便万一被误用立刻暴露。
    #[derive(Default)]
    struct FakeRepo {
        existing: Mutex<Vec<MobileDeviceId>>,
        last_deleted: Mutex<Option<MobileDeviceId>>,
        force_storage_err: bool,
    }

    impl FakeRepo {
        fn with_existing(ids: Vec<&str>) -> Self {
            Self {
                existing: Mutex::new(ids.into_iter().map(MobileDeviceId::new).collect()),
                last_deleted: Mutex::new(None),
                force_storage_err: false,
            }
        }
    }

    #[async_trait]
    impl MobileDeviceRepositoryPort for FakeRepo {
        async fn save(&self, _: &MobileDevice) -> Result<(), MobileDeviceError> {
            unreachable!("revoke 不调用 save")
        }
        async fn find_by_username(
            &self,
            _: &str,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            unreachable!("revoke 不调用 find_by_username")
        }
        async fn find_by_device_id(
            &self,
            _: &MobileDeviceId,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            unreachable!("revoke 不调用 find_by_device_id")
        }
        async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
            unreachable!("revoke 不调用 list_all")
        }
        async fn delete(&self, id: &MobileDeviceId) -> Result<bool, MobileDeviceError> {
            if self.force_storage_err {
                return Err(MobileDeviceError::Storage("disk gone".into()));
            }
            let mut existing = self.existing.lock().unwrap();
            let before = existing.len();
            existing.retain(|x| x.as_str() != id.as_str());
            *self.last_deleted.lock().unwrap() = Some(id.clone());
            Ok(existing.len() < before)
        }
        async fn record_activity(
            &self,
            _: &MobileDeviceId,
            _: i64,
            _: Option<String>,
            _: Option<String>,
            _: Option<String>,
        ) -> Result<(), MobileDeviceError> {
            unreachable!("revoke 不调用 record_activity")
        }
    }

    #[tokio::test]
    async fn deletes_existing_device() {
        let repo = Arc::new(FakeRepo::with_existing(vec!["did_x"]));
        let uc = RevokeMobileDeviceUseCase::new(repo.clone());
        uc.execute(RevokeMobileDeviceInput {
            device_id: MobileDeviceId::new("did_x"),
        })
        .await
        .expect("should succeed");

        assert_eq!(
            repo.last_deleted.lock().unwrap().as_ref().unwrap().as_str(),
            "did_x"
        );
        assert!(repo.existing.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn returns_not_found_when_missing() {
        let repo = Arc::new(FakeRepo::with_existing(vec![]));
        let uc = RevokeMobileDeviceUseCase::new(repo);
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
        let repo = Arc::new(FakeRepo {
            existing: Mutex::new(vec![MobileDeviceId::new("did_x")]),
            last_deleted: Mutex::new(None),
            force_storage_err: true,
        });
        let uc = RevokeMobileDeviceUseCase::new(repo);
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
