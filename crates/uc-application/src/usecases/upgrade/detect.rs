//! [`DetectUpgradeUseCase`] —— 启动期版本游标比较。
//!
//! 判定矩阵（来自 P1 设计共识）：
//!
//! | last_seen_version       | has_completed | 结果                                |
//! |-------------------------|---------------|-------------------------------------|
//! | None                    | false         | FreshInstall                        |
//! | None                    | true          | Upgraded { from: None, to: current }|
//! | Some(v), v == current   | 任意          | NoChange                            |
//! | Some(v), v <  current   | 任意          | Upgraded { from: Some(v), to: current } |
//! | Some(v), v >  current   | 任意          | Downgraded { from: v, to: current } |
//!
//! 解析失败兜底：
//! * 当前版本（构建期常量）解析失败 —— 视作内部错误，返回 [`DetectUpgradeError::CurrentVersionMalformed`]。
//! * 游标版本解析失败 —— 视作"未知旧版本"，归到 `Upgraded { from: None, to: current }`，
//!   并打 warn 日志；与"非 fresh 即老用户"策略一致。

use std::sync::Arc;

use thiserror::Error;
use tracing::{debug, warn};

use uc_core::ports::{AppVersionStateError, AppVersionStatePort, SetupStatusPort};

use super::status::UpgradeStatus;

#[derive(Debug, Error)]
pub(crate) enum DetectUpgradeError {
    #[error("current build version is malformed: {0}")]
    CurrentVersionMalformed(String),

    #[error("read app version cursor failed: {0}")]
    ReadCursor(#[from] AppVersionStateError),

    /// `SetupStatusPort.get_status` 失败。fallback 推断需要它，
    /// 读取失败时返回错误而不是默默走 `FreshInstall`，避免误判老用户为新用户。
    #[error("read setup status failed: {0}")]
    ReadSetupStatus(String),
}

pub(crate) struct DetectUpgradeUseCase {
    app_version_state: Arc<dyn AppVersionStatePort>,
    setup_status: Arc<dyn SetupStatusPort>,
}

impl DetectUpgradeUseCase {
    pub(crate) fn new(
        app_version_state: Arc<dyn AppVersionStatePort>,
        setup_status: Arc<dyn SetupStatusPort>,
    ) -> Self {
        Self {
            app_version_state,
            setup_status,
        }
    }

    /// 执行一次性判定。`current_version_str` 由调用方传入
    /// （通常 = `env!("CARGO_PKG_VERSION")`），保持 use case 不依赖
    /// 构建期常量、利于测试。
    pub(crate) async fn execute(
        &self,
        current_version_str: &str,
    ) -> Result<UpgradeStatus, DetectUpgradeError> {
        let current = semver::Version::parse(current_version_str).map_err(|e| {
            DetectUpgradeError::CurrentVersionMalformed(format!("{current_version_str:?}: {e}"))
        })?;

        let stored = self.app_version_state.read().await?;

        match stored {
            None => {
                let has_completed = self
                    .setup_status
                    .get_status()
                    .await
                    .map_err(|e| DetectUpgradeError::ReadSetupStatus(e.to_string()))?
                    .has_completed;

                if has_completed {
                    debug!(
                        target: "upgrade",
                        current = %current,
                        "no version cursor; setup completed → treating as upgraded from unknown"
                    );
                    Ok(UpgradeStatus::Upgraded {
                        from: None,
                        to: current,
                    })
                } else {
                    debug!(
                        target: "upgrade",
                        current = %current,
                        "no version cursor; setup not completed → fresh install"
                    );
                    Ok(UpgradeStatus::FreshInstall)
                }
            }
            Some(raw) => match semver::Version::parse(&raw) {
                Ok(prev) if prev == current => {
                    debug!(
                        target: "upgrade",
                        current = %current,
                        "cursor matches current version"
                    );
                    Ok(UpgradeStatus::NoChange)
                }
                Ok(prev) if prev < current => {
                    debug!(
                        target: "upgrade",
                        from = %prev,
                        to = %current,
                        "upgrade detected"
                    );
                    Ok(UpgradeStatus::Upgraded {
                        from: Some(prev),
                        to: current,
                    })
                }
                Ok(prev) => {
                    // prev > current —— 回滚。
                    debug!(
                        target: "upgrade",
                        from = %prev,
                        to = %current,
                        "downgrade detected"
                    );
                    Ok(UpgradeStatus::Downgraded {
                        from: prev,
                        to: current,
                    })
                }
                Err(e) => {
                    warn!(
                        target: "upgrade",
                        raw = %raw,
                        error = %e,
                        "cursor content failed to parse as semver; treating as upgrade from unknown"
                    );
                    Ok(UpgradeStatus::Upgraded {
                        from: None,
                        to: current,
                    })
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use uc_core::setup::SetupStatus;

    struct FakeVersionState {
        value: Mutex<Option<String>>,
    }
    impl FakeVersionState {
        fn new(initial: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                value: Mutex::new(initial.map(|s| s.to_string())),
            })
        }
    }
    #[async_trait]
    impl AppVersionStatePort for FakeVersionState {
        async fn read(&self) -> Result<Option<String>, AppVersionStateError> {
            Ok(self.value.lock().unwrap().clone())
        }
        async fn write(&self, version: &str) -> Result<(), AppVersionStateError> {
            *self.value.lock().unwrap() = Some(version.to_string());
            Ok(())
        }
    }

    struct FakeSetupStatus {
        has_completed: bool,
    }
    #[async_trait]
    impl SetupStatusPort for FakeSetupStatus {
        async fn get_status(&self) -> anyhow::Result<SetupStatus> {
            Ok(SetupStatus {
                has_completed: self.has_completed,
                ..SetupStatus::default()
            })
        }
        async fn set_status(&self, _status: &SetupStatus) -> anyhow::Result<()> {
            unreachable!("detect-side test does not write setup status")
        }
    }

    fn build(cursor: Option<&str>, has_completed: bool) -> DetectUpgradeUseCase {
        DetectUpgradeUseCase::new(
            FakeVersionState::new(cursor),
            Arc::new(FakeSetupStatus { has_completed }),
        )
    }

    #[tokio::test]
    async fn no_cursor_and_not_completed_is_fresh_install() {
        let uc = build(None, false);
        assert_eq!(
            uc.execute("1.0.0").await.unwrap(),
            UpgradeStatus::FreshInstall
        );
    }

    #[tokio::test]
    async fn no_cursor_but_setup_completed_is_unknown_upgrade() {
        let uc = build(None, true);
        let to = semver::Version::parse("1.0.0").unwrap();
        assert_eq!(
            uc.execute("1.0.0").await.unwrap(),
            UpgradeStatus::Upgraded { from: None, to }
        );
    }

    #[tokio::test]
    async fn cursor_equal_is_no_change() {
        let uc = build(Some("1.0.0"), true);
        assert_eq!(uc.execute("1.0.0").await.unwrap(), UpgradeStatus::NoChange);
    }

    #[tokio::test]
    async fn cursor_lower_is_upgrade() {
        let uc = build(Some("0.9.3"), true);
        let from = semver::Version::parse("0.9.3").unwrap();
        let to = semver::Version::parse("1.0.0").unwrap();
        assert_eq!(
            uc.execute("1.0.0").await.unwrap(),
            UpgradeStatus::Upgraded {
                from: Some(from),
                to
            }
        );
    }

    #[tokio::test]
    async fn cursor_higher_is_downgrade() {
        let uc = build(Some("1.2.0"), true);
        let from = semver::Version::parse("1.2.0").unwrap();
        let to = semver::Version::parse("1.0.0").unwrap();
        assert_eq!(
            uc.execute("1.0.0").await.unwrap(),
            UpgradeStatus::Downgraded { from, to }
        );
    }

    #[tokio::test]
    async fn malformed_cursor_falls_back_to_unknown_upgrade() {
        let uc = build(Some("garbage-not-semver"), true);
        let to = semver::Version::parse("1.0.0").unwrap();
        assert_eq!(
            uc.execute("1.0.0").await.unwrap(),
            UpgradeStatus::Upgraded { from: None, to }
        );
    }

    #[tokio::test]
    async fn malformed_current_is_internal_error() {
        let uc = build(None, false);
        let err = uc.execute("not-a-version").await.unwrap_err();
        assert!(matches!(
            err,
            DetectUpgradeError::CurrentVersionMalformed(_)
        ));
    }

    #[tokio::test]
    async fn prerelease_ordering_matches_semver_spec() {
        // 1.0.0-alpha.1 < 1.0.0 (semver §11)
        let uc = build(Some("1.0.0-alpha.1"), true);
        let from = semver::Version::parse("1.0.0-alpha.1").unwrap();
        let to = semver::Version::parse("1.0.0").unwrap();
        assert_eq!(
            uc.execute("1.0.0").await.unwrap(),
            UpgradeStatus::Upgraded {
                from: Some(from),
                to
            }
        );
    }
}
