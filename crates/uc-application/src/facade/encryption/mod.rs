use std::sync::Arc;

use tracing::instrument;
use uc_core::crypto::model::Passphrase;
use uc_core::ids::SpaceId;
use uc_core::ports::setup::SetupStatusPort;
use uc_core::ports::space::{
    InitializeSpacePort, IsSpaceUnlockedPort, LockSpacePort, ResumeSpaceSessionPort,
    SpaceAccessError, VerifyKeychainAccessPort,
};

const DEFAULT_SPACE_ID: &str = "space";

/// Narrow space-access ports consumed by [`EncryptionFacade`]. Each maps to one
/// facade method; the facade holds only the slices it calls (ports.md §8.1).
#[derive(Clone)]
pub struct EncryptionFacadeDeps {
    pub setup_status: Arc<dyn SetupStatusPort>,
    pub initialize: Arc<dyn InitializeSpacePort>,
    pub resume_session: Arc<dyn ResumeSpaceSessionPort>,
    pub is_unlocked: Arc<dyn IsSpaceUnlockedPort>,
    pub lock: Arc<dyn LockSpacePort>,
    pub verify_keychain_access: Arc<dyn VerifyKeychainAccessPort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionStateView {
    pub initialized: bool,
    pub session_ready: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum EncryptionFacadeError {
    #[error("failed to load setup status: {0}")]
    SetupStatus(String),
    #[error("space access failed: {0}")]
    SpaceAccess(String),
    #[error("encryption is already initialized")]
    AlreadyInitialized,
}

pub struct EncryptionFacade {
    deps: EncryptionFacadeDeps,
}

impl EncryptionFacade {
    pub fn new(deps: EncryptionFacadeDeps) -> Self {
        Self { deps }
    }

    // 故意不挂 `#[instrument]`:`state()` 是高频只读查询(前端轮询 +
    // CLI / daemon 每个请求都会读一次),自身不做 I/O,也不推进流程。
    // 给它开 span 会被 sentry-tracing 上报成 transaction —— 14 天观测到
    // 96 万次,占 span 配额 ~27%。如要排障,出错路径用 `tracing::warn!`
    // / `error!` 即可,无需 root span。其他写动作(initialize / unlock /
    // lock / verify_keychain_access)仍保留 instrument。
    pub async fn state(&self) -> Result<EncryptionStateView, EncryptionFacadeError> {
        let initialized = self
            .deps
            .setup_status
            .get_status()
            .await
            .map(|status| status.has_completed)
            .map_err(|err| EncryptionFacadeError::SetupStatus(err.to_string()))?;
        let space_id = default_space_id();
        let session_ready = if initialized {
            self.deps.is_unlocked.is_unlocked(&space_id).await
        } else {
            false
        };

        Ok(EncryptionStateView {
            initialized,
            session_ready,
        })
    }

    /// 首次初始化加密空间 —— 创建本地 space + 解锁 + 标记 setup 完成。
    ///
    /// 三步合并为单一原子动作:
    /// 1. `space_access.initialize` 用 passphrase 创建本地空间并解锁会话
    /// 2. `setup_status.set_status(has_completed=true)` 标记本设备已完成 setup
    ///
    /// 第 1 步失败立即返回错误,不写 setup_status。
    #[instrument(skip_all)]
    pub async fn initialize(&self, passphrase: Passphrase) -> Result<(), EncryptionFacadeError> {
        let space_id = default_space_id();
        let domain_passphrase = uc_core::crypto::domain::Passphrase::new(passphrase.0);

        self.deps
            .initialize
            .initialize(&space_id, &domain_passphrase)
            .await
            .map_err(|err| match err {
                SpaceAccessError::AlreadyInitialized => EncryptionFacadeError::AlreadyInitialized,
                other => space_access_error(other),
            })?;

        let mut status = self
            .deps
            .setup_status
            .get_status()
            .await
            .map_err(|err| EncryptionFacadeError::SetupStatus(err.to_string()))?;
        status.has_completed = true;
        self.deps
            .setup_status
            .set_status(&status)
            .await
            .map_err(|err| EncryptionFacadeError::SetupStatus(err.to_string()))?;

        Ok(())
    }

    #[instrument(skip_all)]
    pub async fn unlock(&self) -> Result<bool, EncryptionFacadeError> {
        match self
            .deps
            .resume_session
            .try_resume_session(&default_space_id())
            .await
        {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(err) => Err(space_access_error(err)),
        }
    }

    #[instrument(skip_all)]
    pub async fn lock(&self) -> Result<(), EncryptionFacadeError> {
        self.deps
            .lock
            .lock(&default_space_id())
            .await
            .map_err(space_access_error)
    }

    #[instrument(skip_all)]
    pub async fn verify_keychain_access(&self) -> Result<bool, EncryptionFacadeError> {
        self.deps
            .verify_keychain_access
            .verify_keychain_access()
            .await
            .map_err(space_access_error)
    }
}

fn default_space_id() -> SpaceId {
    SpaceId::from(DEFAULT_SPACE_ID)
}

fn space_access_error(err: SpaceAccessError) -> EncryptionFacadeError {
    EncryptionFacadeError::SpaceAccess(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use std::sync::Mutex;
    use uc_core::crypto::domain::{ActiveSpace, Passphrase};
    use uc_core::setup::SetupStatus;

    #[derive(Default)]
    struct FakeSetupStatus {
        status: Mutex<SetupStatus>,
    }

    #[async_trait]
    impl SetupStatusPort for FakeSetupStatus {
        async fn get_status(&self) -> anyhow::Result<SetupStatus> {
            Ok(self.status.lock().expect("status lock").clone())
        }

        async fn set_status(&self, status: &SetupStatus) -> anyhow::Result<()> {
            *self.status.lock().expect("status lock") = status.clone();
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeSpaceAccess {
        unlocked: Mutex<bool>,
        resume_returns_session: Mutex<bool>,
        verify_granted: Mutex<bool>,
        lock_calls: Mutex<u32>,
        init_already_initialized: Mutex<bool>,
        init_calls: Mutex<u32>,
    }

    #[async_trait]
    impl InitializeSpacePort for FakeSpaceAccess {
        async fn initialize(
            &self,
            space_id: &SpaceId,
            _passphrase: &Passphrase,
        ) -> Result<ActiveSpace, SpaceAccessError> {
            *self.init_calls.lock().expect("init calls lock") += 1;
            if *self.init_already_initialized.lock().expect("init flag") {
                return Err(SpaceAccessError::AlreadyInitialized);
            }
            Ok(ActiveSpace::new(space_id.clone()))
        }
    }

    #[async_trait]
    impl IsSpaceUnlockedPort for FakeSpaceAccess {
        async fn is_unlocked(&self, _space_id: &SpaceId) -> bool {
            *self.unlocked.lock().expect("unlocked lock")
        }
    }

    #[async_trait]
    impl LockSpacePort for FakeSpaceAccess {
        async fn lock(&self, _space_id: &SpaceId) -> Result<(), SpaceAccessError> {
            *self.unlocked.lock().expect("unlocked lock") = false;
            *self.lock_calls.lock().expect("lock calls lock") += 1;
            Ok(())
        }
    }

    #[async_trait]
    impl ResumeSpaceSessionPort for FakeSpaceAccess {
        async fn try_resume_session(
            &self,
            space_id: &SpaceId,
        ) -> Result<Option<ActiveSpace>, SpaceAccessError> {
            if *self.resume_returns_session.lock().expect("resume lock") {
                *self.unlocked.lock().expect("unlocked lock") = true;
                Ok(Some(ActiveSpace::new(space_id.clone())))
            } else {
                Ok(None)
            }
        }
    }

    #[async_trait]
    impl VerifyKeychainAccessPort for FakeSpaceAccess {
        async fn verify_keychain_access(&self) -> Result<bool, SpaceAccessError> {
            Ok(*self.verify_granted.lock().expect("verify lock"))
        }
    }

    fn facade_with(
        completed: bool,
        unlocked: bool,
        resume_returns_session: bool,
        verify_granted: bool,
    ) -> (EncryptionFacade, Arc<FakeSpaceAccess>) {
        let setup_status = Arc::new(FakeSetupStatus::default());
        setup_status
            .status
            .lock()
            .expect("status lock")
            .has_completed = completed;
        let space_access = Arc::new(FakeSpaceAccess::default());
        *space_access.unlocked.lock().expect("unlocked lock") = unlocked;
        *space_access
            .resume_returns_session
            .lock()
            .expect("resume lock") = resume_returns_session;
        *space_access.verify_granted.lock().expect("verify lock") = verify_granted;

        (
            EncryptionFacade::new(EncryptionFacadeDeps {
                setup_status,
                initialize: space_access.clone(),
                resume_session: space_access.clone(),
                is_unlocked: space_access.clone(),
                lock: space_access.clone(),
                verify_keychain_access: space_access.clone(),
            }),
            space_access,
        )
    }

    #[tokio::test]
    async fn state_reports_not_ready_when_setup_is_incomplete() {
        let (facade, _) = facade_with(false, true, false, false);

        let state = facade.state().await.expect("state");

        assert_eq!(
            state,
            EncryptionStateView {
                initialized: false,
                session_ready: false
            }
        );
    }

    #[tokio::test]
    async fn state_reports_session_ready_after_completed_setup() {
        let (facade, _) = facade_with(true, true, false, false);

        let state = facade.state().await.expect("state");

        assert_eq!(
            state,
            EncryptionStateView {
                initialized: true,
                session_ready: true
            }
        );
    }

    #[tokio::test]
    async fn unlock_returns_whether_session_was_resumed() {
        let (resumed, _) = facade_with(true, false, true, false);
        let (not_resumed, _) = facade_with(true, false, false, false);

        assert!(resumed.unlock().await.expect("resumed"));
        assert!(!not_resumed.unlock().await.expect("not resumed"));
    }

    #[tokio::test]
    async fn lock_clears_session() {
        let (facade, space_access) = facade_with(true, true, false, false);

        facade.lock().await.expect("lock");

        assert!(!*space_access.unlocked.lock().expect("unlocked lock"));
        assert_eq!(*space_access.lock_calls.lock().expect("lock calls lock"), 1);
    }

    #[tokio::test]
    async fn verify_keychain_access_returns_grant_state() {
        let (facade, _) = facade_with(true, false, false, true);

        assert!(facade
            .verify_keychain_access()
            .await
            .expect("verify keychain"));
    }

    #[tokio::test]
    async fn initialize_creates_space_and_marks_setup_complete() {
        use uc_core::crypto::model::Passphrase as ModelPassphrase;

        let (facade, space_access) = facade_with(false, false, false, false);

        facade
            .initialize(ModelPassphrase("hunter2".to_string()))
            .await
            .expect("initialize");

        assert_eq!(*space_access.init_calls.lock().expect("init calls"), 1);
        let state = facade.state().await.expect("state");
        assert!(state.initialized);
    }

    #[tokio::test]
    async fn initialize_maps_already_initialized_error() {
        use uc_core::crypto::model::Passphrase as ModelPassphrase;

        let (facade, space_access) = facade_with(false, false, false, false);
        *space_access.init_already_initialized.lock().expect("flag") = true;

        let err = facade
            .initialize(ModelPassphrase("hunter2".to_string()))
            .await
            .expect_err("initialize should fail");

        assert!(matches!(err, EncryptionFacadeError::AlreadyInitialized));
        // setup_status must remain unchanged when initialization aborts.
        let state = facade.state().await.expect("state");
        assert!(!state.initialized);
    }
}
