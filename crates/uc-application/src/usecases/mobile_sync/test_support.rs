//! mobile_sync use case 单元测试共享的 mockall mocks 与 fake sinks。
//!
//! ## 这里放什么
//!
//! 在多个 use case 测试里**重复出现**的 mock,集中一次写完,所有测试 `use
//! super::test_support::*;` 即可。当前承载:
//!
//! - [`MockDeviceRepo`] —— [`MobileDeviceRepositoryPort`] 的 mockall mock,
//!   被 register / authenticate / update / revoke / list 共用。
//! - [`MockHasher`] —— [`PasswordHasherPort`] 的 mockall mock,被 register /
//!   authenticate / update 共用。
//! - [`MockMinter`] —— [`MobileCredentialsMinterPort`] 的 mockall mock,被
//!   register / update 共用。
//! - [`MockStaging`] —— [`MobileFileStagingPort`] 的 mockall mock,被
//!   apply_incoming / get_file 共用。
//! - [`CapturingAnalyticsSink`] —— "录制全部 captured Event" 的 fake
//!   [`AnalyticsPort`],被 register / authenticate / apply_incoming 共用,
//!   并预留给 mobile_sync 内未来新增的 analytics 断言测试。
//!
//! ## 这里**不**放什么
//!
//! - 只在单一文件里用一次的 mock(如 register_device 的 `MockSettings` /
//!   `MockClock` / `MockProbe`):保持局部性,避免抽出"只一处用"的 mock
//!   反而增加查找成本。
//! - 有状态 fake(`InMemorySettings` / `FakeEntryRepo` / `FixedProbe` 等):
//!   它们的"内存型实现"语义比 mockall 的 expectation 语义更适合,刻意保留
//!   在原文件。
//!
//! ## 为什么 [`CapturingAnalyticsSink`] 不用 mockall
//!
//! mockall 是 "expectation" 模型:测试前置 expect,事后由 mockall 在 drop 时
//! verify。而 analytics 断言多采用 "先做事再 `assert_eq!(events(), vec![…])`"
//! 录制模型,后者直接、可读、对 event 顺序敏感,手写 sink 比迁就 mockall
//! 更合适。

#![cfg(test)]

use std::sync::Mutex;

use async_trait::async_trait;

use uc_core::mobile_sync::{
    MintedCredentials, MobileDevice, MobileDeviceError, MobileDeviceId, StagedFile, StagingHandle,
};
use uc_core::ports::mobile_sync::{MobileFileStagingError, MobileFileStagingPort};
use uc_core::ports::{
    MobileCredentialsMinterPort, MobileDeviceRepositoryPort, PasswordHasherError,
    PasswordHasherPort,
};
use uc_observability::analytics::{AnalyticsPort, Event};

mockall::mock! {
    pub DeviceRepo {}
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
        async fn update_mobile_device(
            &self,
            updated: &MobileDevice,
        ) -> Result<bool, MobileDeviceError>;
    }
}

mockall::mock! {
    pub Hasher {}
    #[async_trait]
    impl PasswordHasherPort for Hasher {
        async fn hash(&self, password: &str) -> Result<String, PasswordHasherError>;
        async fn verify(&self, password: &str, phc: &str)
            -> Result<bool, PasswordHasherError>;
    }
}

mockall::mock! {
    pub Minter {}
    impl MobileCredentialsMinterPort for Minter {
        fn mint_credentials(&self) -> MintedCredentials;
    }
}

mockall::mock! {
    pub Staging {}
    #[async_trait]
    impl MobileFileStagingPort for Staging {
        async fn stage_file(
            &self,
            scope_id: &str,
            data_name: &str,
            mime: &str,
            bytes: Vec<u8>,
        ) -> Result<StagedFile, MobileFileStagingError>;
        async fn read_by_uri(&self, uri: &str) -> Result<Vec<u8>, MobileFileStagingError>;
        async fn begin_stage(
            &self,
            scope_id: &str,
            data_name: &str,
            mime: &str,
        ) -> Result<StagingHandle, MobileFileStagingError>;
        async fn append_stage_chunk(
            &self,
            handle: &StagingHandle,
            chunk: &[u8],
        ) -> Result<(), MobileFileStagingError>;
        async fn finalize_stage(
            &self,
            handle: StagingHandle,
        ) -> Result<StagedFile, MobileFileStagingError>;
        async fn abort_stage(&self, handle: StagingHandle);
    }
}

/// 录制所有 captured [`Event`] 的 fake [`AnalyticsPort`]。测试侧调用 use case
/// 后用 [`events`](Self::events) 取出快照,做 `assert_eq!` 即可。
///
/// 选用手写而非 mockall 的理由见模块顶部说明。
#[derive(Default)]
pub struct CapturingAnalyticsSink {
    captured: Mutex<Vec<Event>>,
}

impl CapturingAnalyticsSink {
    pub fn events(&self) -> Vec<Event> {
        self.captured.lock().unwrap().clone()
    }
}

impl AnalyticsPort for CapturingAnalyticsSink {
    fn capture(&self, event: Event) {
        self.captured.lock().unwrap().push(event);
    }
}
