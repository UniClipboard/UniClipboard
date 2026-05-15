//! A2 · `UnlockSpaceUseCase`.
//!
//! Post-setup start-up flow: check `SetupStatus.has_completed`, then
//! forward to `SpaceAccessPort::unlock`. Because A1 is atomic, if we
//! ever reach A2 we can assume the owner `SpaceMember` / identity are
//! already persisted — A2 does not do a "self-member self-heal" round
//! (decision from Slice 1 outside-in session).

use std::sync::Arc;

use tracing::{debug, info, instrument, warn};

use uc_core::ids::SpaceId;
use uc_core::ports::space::{SpaceAccessError, SpaceAccessPort};
use uc_core::ports::SetupStatusPort;
use uc_observability::analytics::{AnalyticsPort, Event, UnlockFailureReason};

use crate::facade::space_setup::commands::UnlockSpaceCommand;
use crate::facade::space_setup::{UnlockSpaceError, UnlockSpaceResult};

pub(crate) struct UnlockSpaceUseCase {
    space_access: Arc<dyn SpaceAccessPort>,
    setup_status: Arc<dyn SetupStatusPort>,
    /// schema doc §12.1 · 每次 daemon 重启的可靠性 anchor。成功路径发
    /// `space_unlocked`，失败路径按 `UnlockFailureReason` 映射发
    /// `space_unlock_failed`。pre-condition 失败（`SetupNotCompleted`）
    /// 不属于"用户能不能继续用产品"语义，**不**上报。
    analytics: Arc<dyn AnalyticsPort>,
}

impl UnlockSpaceUseCase {
    pub(crate) fn new(
        space_access: Arc<dyn SpaceAccessPort>,
        setup_status: Arc<dyn SetupStatusPort>,
        analytics: Arc<dyn AnalyticsPort>,
    ) -> Self {
        Self {
            space_access,
            setup_status,
            analytics,
        }
    }

    #[instrument(skip(self, cmd))]
    pub(crate) async fn execute(
        &self,
        cmd: UnlockSpaceCommand,
    ) -> Result<UnlockSpaceResult, UnlockSpaceError> {
        let status = match self.setup_status.get_status().await {
            Ok(s) => s,
            Err(e) => {
                // status 读取失败属于"用户点 unlock 失败、无法继续用产品"——
                // 应计入可靠性 anchor，归类 Internal。
                self.analytics.capture(Event::SpaceUnlockFailed {
                    failure_reason: UnlockFailureReason::Internal,
                });
                return Err(UnlockSpaceError::Internal(e.to_string()));
            }
        };
        if !status.has_completed {
            debug!("unlock rejected: setup not completed");
            // 上层调用错误（用户应该走 setup 而不是 unlock），不算 unlock
            // 失败。dashboard 看到 unlock_failed 即代表 setup 已完成但仍解锁
            // 不开，这是真实可靠性信号。
            return Err(UnlockSpaceError::SetupNotCompleted);
        }

        let space_id = status.space_id.clone().unwrap_or_else(SpaceId::new);
        match self.space_access.unlock(&space_id, &cmd.passphrase).await {
            Ok(_) => {
                info!(%space_id, "space unlocked");
                self.analytics.capture(Event::SpaceUnlocked);
                Ok(UnlockSpaceResult { space_id })
            }
            Err(err) => {
                self.analytics.capture(Event::SpaceUnlockFailed {
                    failure_reason: unlock_failure_reason(&err),
                });
                Err(map_unlock_err(err))
            }
        }
    }
}

/// 把 `SpaceAccessError` 映射到 telemetry 的 `UnlockFailureReason`。
///
/// 与 [`map_unlock_err`] 输出独立——前者是产品分析口径，后者是面向上层
/// 的应用错误。`KeyringUnavailable` 暂无独立 `SpaceAccessError` variant，
/// 未来 adapter 暴露专用错误码时再细分；当前与其它内部错误一起归 `Internal`。
fn unlock_failure_reason(err: &SpaceAccessError) -> UnlockFailureReason {
    match err {
        SpaceAccessError::WrongPassphrase => UnlockFailureReason::PassphraseMismatch,
        SpaceAccessError::NotInitialized => UnlockFailureReason::SpaceNotFound,
        SpaceAccessError::CorruptedKeyMaterial => UnlockFailureReason::KeyslotCorrupted,
        _ => UnlockFailureReason::Internal,
    }
}

fn map_unlock_err(err: SpaceAccessError) -> UnlockSpaceError {
    match err {
        SpaceAccessError::NotInitialized => UnlockSpaceError::SpaceNotInitialized,
        SpaceAccessError::WrongPassphrase => UnlockSpaceError::WrongPassphrase,
        SpaceAccessError::CorruptedKeyMaterial => UnlockSpaceError::CorruptedKeyMaterial,
        SpaceAccessError::Internal(m) => UnlockSpaceError::Internal(m),
        other => {
            warn!(error = %other, "unexpected space access error during unlock");
            UnlockSpaceError::Internal(other.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;

    use uc_core::crypto::domain::{ActiveSpace, Passphrase};
    use uc_core::ids::SpaceId;
    use uc_core::ports::space::{SpaceAccessError, SpaceAccessPort};
    use uc_core::setup::SetupStatus;
    use uc_core::space_access::{JoinOffer, ProofDerivedKey};

    #[derive(Default)]
    struct FakeSpaceAccess {
        unlock_err: Mutex<Option<SpaceAccessError>>,
        unlock_calls: Mutex<u32>,
    }
    #[async_trait]
    impl SpaceAccessPort for FakeSpaceAccess {
        async fn initialize(
            &self,
            _space_id: &SpaceId,
            _passphrase: &Passphrase,
        ) -> Result<ActiveSpace, SpaceAccessError> {
            unimplemented!("A2 test does not touch initialize")
        }
        async fn unlock(
            &self,
            space_id: &SpaceId,
            _passphrase: &Passphrase,
        ) -> Result<ActiveSpace, SpaceAccessError> {
            *self.unlock_calls.lock().unwrap() += 1;
            if let Some(err) = self.unlock_err.lock().unwrap().take() {
                return Err(err);
            }
            Ok(ActiveSpace::new(space_id.clone()))
        }
        async fn is_unlocked(&self, _space_id: &SpaceId) -> bool {
            true
        }
        async fn lock(&self, _space_id: &SpaceId) -> Result<(), SpaceAccessError> {
            Ok(())
        }
        async fn factory_reset(&self, _space_id: &SpaceId) -> Result<(), SpaceAccessError> {
            Ok(())
        }
        async fn try_resume_session(
            &self,
            _space_id: &SpaceId,
        ) -> Result<Option<ActiveSpace>, SpaceAccessError> {
            Ok(None)
        }
        async fn verify_keychain_access(&self) -> Result<bool, SpaceAccessError> {
            Ok(true)
        }
        async fn derive_subkey(
            &self,
            _salt: &[u8],
            _info: &[u8],
        ) -> Result<[u8; 32], SpaceAccessError> {
            Ok([0; 32])
        }
        async fn current_session_proof_key(
            &self,
        ) -> Result<Option<ProofDerivedKey>, SpaceAccessError> {
            Ok(None)
        }
        async fn prepare_join_offer(
            &self,
            _space_id: &SpaceId,
            _passphrase: &Passphrase,
        ) -> Result<JoinOffer, SpaceAccessError> {
            unimplemented!()
        }
        async fn derive_master_key_for_proof(
            &self,
            _offer: &JoinOffer,
            _passphrase: &Passphrase,
        ) -> Result<ProofDerivedKey, SpaceAccessError> {
            unimplemented!()
        }
    }

    #[derive(Default)]
    struct InMemorySetupStatus {
        status: Mutex<SetupStatus>,
    }
    #[async_trait]
    impl SetupStatusPort for InMemorySetupStatus {
        async fn get_status(&self) -> anyhow::Result<SetupStatus> {
            Ok(self.status.lock().unwrap().clone())
        }
        async fn set_status(&self, status: &SetupStatus) -> anyhow::Result<()> {
            *self.status.lock().unwrap() = status.clone();
            Ok(())
        }
    }

    #[derive(Default)]
    struct CapturingAnalyticsSink {
        captured: Mutex<Vec<Event>>,
    }
    impl CapturingAnalyticsSink {
        fn events(&self) -> Vec<Event> {
            self.captured.lock().unwrap().clone()
        }
    }
    impl AnalyticsPort for CapturingAnalyticsSink {
        fn capture(&self, event: Event) {
            self.captured.lock().unwrap().push(event);
        }
    }

    fn build(
        setup_completed: bool,
        unlock_err: Option<SpaceAccessError>,
    ) -> (
        UnlockSpaceUseCase,
        Arc<FakeSpaceAccess>,
        Arc<CapturingAnalyticsSink>,
        Option<SpaceId>,
    ) {
        let space_access = Arc::new(FakeSpaceAccess::default());
        *space_access.unlock_err.lock().unwrap() = unlock_err;
        let setup_status = Arc::new(InMemorySetupStatus::default());
        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let seeded_space_id = if setup_completed {
            let id = SpaceId::new();
            let mut guard = setup_status.status.lock().unwrap();
            guard.has_completed = true;
            guard.space_id = Some(id.clone());
            Some(id)
        } else {
            None
        };
        let uc = UnlockSpaceUseCase::new(space_access.clone(), setup_status, analytics.clone());
        (uc, space_access, analytics, seeded_space_id)
    }

    fn cmd(pass: &str) -> UnlockSpaceCommand {
        UnlockSpaceCommand {
            passphrase: Passphrase::new(pass),
        }
    }

    #[tokio::test]
    async fn happy_path_returns_space_id_from_setup_status() {
        let (uc, sa, analytics, seeded) = build(true, None);
        let r = uc.execute(cmd("pass")).await.unwrap();
        assert_eq!(r.space_id, seeded.expect("seeded"));
        assert_eq!(*sa.unlock_calls.lock().unwrap(), 1);
        // schema doc §12.1 · 成功路径必发 space_unlocked。
        let events = analytics.events();
        assert_eq!(events, vec![Event::SpaceUnlocked]);
    }

    /// Legacy profiles pre-dating F-058 may have `space_id == None` in
    /// `SetupStatus`. Spec: fall back to minting a fresh id so A2 is not
    /// blocked. T-17 self-heal is explicitly out of scope (backlog).
    #[tokio::test]
    async fn missing_setup_space_id_falls_back_to_fresh_mint() {
        let space_access = Arc::new(FakeSpaceAccess::default());
        let setup_status = Arc::new(InMemorySetupStatus::default());
        setup_status.status.lock().unwrap().has_completed = true;
        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let uc = UnlockSpaceUseCase::new(space_access.clone(), setup_status, analytics.clone());
        let r = uc.execute(cmd("pass")).await.unwrap();
        assert!(!r.space_id.inner().is_empty());
        assert_eq!(*space_access.unlock_calls.lock().unwrap(), 1);
        assert_eq!(analytics.events(), vec![Event::SpaceUnlocked]);
    }

    #[tokio::test]
    async fn setup_not_completed_short_circuits_before_unlock() {
        let (uc, sa, analytics, _) = build(false, None);
        let err = uc.execute(cmd("pass")).await.unwrap_err();
        assert!(matches!(err, UnlockSpaceError::SetupNotCompleted));
        assert_eq!(*sa.unlock_calls.lock().unwrap(), 0);
        // pre-condition mismatch 不属于可靠性 anchor 语义，不应 emit。
        assert!(analytics.events().is_empty(), "{:?}", analytics.events());
    }

    #[tokio::test]
    async fn wrong_passphrase_maps_to_specific_variant() {
        let (uc, _sa, analytics, _) = build(true, Some(SpaceAccessError::WrongPassphrase));
        let err = uc.execute(cmd("wrong")).await.unwrap_err();
        assert!(matches!(err, UnlockSpaceError::WrongPassphrase));
        assert_eq!(
            analytics.events(),
            vec![Event::SpaceUnlockFailed {
                failure_reason: UnlockFailureReason::PassphraseMismatch,
            }]
        );
    }

    #[tokio::test]
    async fn not_initialized_maps_to_space_not_initialized() {
        let (uc, _sa, analytics, _) = build(true, Some(SpaceAccessError::NotInitialized));
        let err = uc.execute(cmd("pass")).await.unwrap_err();
        assert!(matches!(err, UnlockSpaceError::SpaceNotInitialized));
        assert_eq!(
            analytics.events(),
            vec![Event::SpaceUnlockFailed {
                failure_reason: UnlockFailureReason::SpaceNotFound,
            }]
        );
    }

    #[tokio::test]
    async fn corrupted_key_material_maps_to_specific_variant() {
        let (uc, _sa, analytics, _) = build(true, Some(SpaceAccessError::CorruptedKeyMaterial));
        let err = uc.execute(cmd("pass")).await.unwrap_err();
        assert!(matches!(err, UnlockSpaceError::CorruptedKeyMaterial));
        assert_eq!(
            analytics.events(),
            vec![Event::SpaceUnlockFailed {
                failure_reason: UnlockFailureReason::KeyslotCorrupted,
            }]
        );
    }

    #[tokio::test]
    async fn internal_error_passthrough() {
        let (uc, _sa, analytics, _) = build(true, Some(SpaceAccessError::Internal("boom".into())));
        let err = uc.execute(cmd("pass")).await.unwrap_err();
        match err {
            UnlockSpaceError::Internal(m) => assert_eq!(m, "boom"),
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            analytics.events(),
            vec![Event::SpaceUnlockFailed {
                failure_reason: UnlockFailureReason::Internal,
            }]
        );
    }
}
