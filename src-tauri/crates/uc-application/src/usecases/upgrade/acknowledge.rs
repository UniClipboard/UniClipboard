//! [`AcknowledgeUseCase`] —— 把版本游标推进到当前版本。
//!
//! 调用方（UI 弹窗确认 / CLI `--acknowledge` / 自动确认）执行后，
//! 下次启动 `DetectUpgradeUseCase` 将得到 `UpgradeStatus::NoChange`。

use std::sync::Arc;

use thiserror::Error;
use tracing::info;

use uc_core::ports::{AppVersionStateError, AppVersionStatePort};

#[derive(Debug, Error)]
pub(crate) enum AcknowledgeError {
    #[error("current build version is malformed: {0}")]
    CurrentVersionMalformed(String),

    #[error("write app version cursor failed: {0}")]
    WriteCursor(#[from] AppVersionStateError),
}

pub(crate) struct AcknowledgeUseCase {
    app_version_state: Arc<dyn AppVersionStatePort>,
}

impl AcknowledgeUseCase {
    pub(crate) fn new(app_version_state: Arc<dyn AppVersionStatePort>) -> Self {
        Self { app_version_state }
    }

    /// 把游标推进到 `current_version_str`。先用 semver 校验合法性，
    /// 避免把无效字符串写回磁盘污染游标。
    pub(crate) async fn execute(&self, current_version_str: &str) -> Result<(), AcknowledgeError> {
        let _validated = semver::Version::parse(current_version_str).map_err(|e| {
            AcknowledgeError::CurrentVersionMalformed(format!("{current_version_str:?}: {e}"))
        })?;

        self.app_version_state.write(current_version_str).await?;
        info!(
            target: "upgrade",
            version = %current_version_str,
            "app version cursor advanced"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct FakeVersionState {
        value: Mutex<Option<String>>,
        write_should_fail: bool,
    }
    impl FakeVersionState {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                value: Mutex::new(None),
                write_should_fail: false,
            })
        }
        fn failing() -> Arc<Self> {
            Arc::new(Self {
                value: Mutex::new(None),
                write_should_fail: true,
            })
        }
    }
    #[async_trait]
    impl AppVersionStatePort for FakeVersionState {
        async fn read(&self) -> Result<Option<String>, AppVersionStateError> {
            Ok(self.value.lock().unwrap().clone())
        }
        async fn write(&self, version: &str) -> Result<(), AppVersionStateError> {
            if self.write_should_fail {
                return Err(AppVersionStateError::Write("simulated".into()));
            }
            *self.value.lock().unwrap() = Some(version.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn acknowledge_writes_cursor_and_round_trips() {
        let port = FakeVersionState::new();
        let uc = AcknowledgeUseCase::new(port.clone());
        uc.execute("1.0.0-alpha.1").await.unwrap();
        assert_eq!(port.value.lock().unwrap().as_deref(), Some("1.0.0-alpha.1"));
    }

    #[tokio::test]
    async fn acknowledge_rejects_invalid_version() {
        let uc = AcknowledgeUseCase::new(FakeVersionState::new());
        let err = uc.execute("not-semver").await.unwrap_err();
        assert!(matches!(err, AcknowledgeError::CurrentVersionMalformed(_)));
    }

    #[tokio::test]
    async fn acknowledge_propagates_write_failure() {
        let uc = AcknowledgeUseCase::new(FakeVersionState::failing());
        let err = uc.execute("1.0.0").await.unwrap_err();
        assert!(matches!(err, AcknowledgeError::WriteCursor(_)));
    }
}
