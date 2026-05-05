//! `UpdateMobileSyncSettingsUseCase` —— 持久化用户对移动端同步设置的修改。
//!
//! v1 仅支持修改 `enabled` 字段（其它字段都是常量推导，详见 SPEC §13 与
//! `get_settings.rs` 的模块文档）。修改后由调用方决定何时重启 daemon —— use
//! case 自身不触发 daemon 重启，只在 [`UpdateMobileSyncSettingsOutput`] 上
//! 用 `restart_required` 标志告知"这次改动需要重启才能生效"。
//!
//! 三个细节值得记下来：
//!
//! 1. **load → mutate → save 的原子性是 SettingsPort 适配器的职责**。本
//!    use case 不持锁也不重读校验：现有 `SettingsPort` 实现是单写者
//!    (daemon 进程独占)，并发竞态非问题。如果将来引入 multi-writer，需要
//!    在 SettingsPort 层提供 CAS 或事务能力，而不是在 use case 里"再 load
//!    一遍"做乐观比较 —— 那只会被认为安全实则有 ABA。
//!
//! 2. **`restart_required` 的判定一律基于"有效变更"**。即只有 `enabled`
//!    实际从 X→¬X 才置 `true`；否则即便用户重复点了同一个值也不要求重启
//!    —— 否则前端会在所有等幂操作后弹"请重启"。
//!
//! 3. **没有 `dry_run` 选项**：v1 设置项只有一个开关，UI 直接保存即可，
//!    无需先预演。

use std::sync::Arc;

use tracing::instrument;

use uc_core::ports::SettingsPort;

// ─── public-shaped (input / output / error) ─────────────────────────────

#[derive(Debug, Clone)]
pub struct UpdateMobileSyncSettingsInput {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateMobileSyncSettingsOutput {
    /// 落盘后的 `enabled` 值（永远等于 input.enabled，便于调用方拼 UI）。
    pub enabled: bool,
    /// 本次保存是否带来了"需要重启 daemon 才能生效"的影响。
    /// 只有 `enabled` 实际变化时为 `true`；同值重复保存为 `false`。
    pub restart_required: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateMobileSyncSettingsError {
    #[error("settings load failed: {0}")]
    SettingsLoadFailed(String),

    #[error("settings save failed: {0}")]
    SettingsSaveFailed(String),
}

// ─── use case ───────────────────────────────────────────────────────────

pub(crate) struct UpdateMobileSyncSettingsUseCase {
    settings: Arc<dyn SettingsPort>,
}

impl UpdateMobileSyncSettingsUseCase {
    pub(crate) fn new(settings: Arc<dyn SettingsPort>) -> Self {
        Self { settings }
    }

    #[instrument(skip(self), fields(enabled = input.enabled))]
    pub(crate) async fn execute(
        &self,
        input: UpdateMobileSyncSettingsInput,
    ) -> Result<UpdateMobileSyncSettingsOutput, UpdateMobileSyncSettingsError> {
        let mut current =
            self.settings.load().await.map_err(|err| {
                UpdateMobileSyncSettingsError::SettingsLoadFailed(err.to_string())
            })?;

        let previous_enabled = current.mobile_sync.enabled;
        let restart_required = previous_enabled != input.enabled;

        if restart_required {
            current.mobile_sync.enabled = input.enabled;
            self.settings.save(&current).await.map_err(|err| {
                UpdateMobileSyncSettingsError::SettingsSaveFailed(err.to_string())
            })?;
        }
        // 同值时跳过 save —— 避免 mtime / 文件系统副作用，也避免上层 watcher
        // 收到无意义的 settings-changed 事件。

        Ok(UpdateMobileSyncSettingsOutput {
            enabled: input.enabled,
            restart_required,
        })
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;

    use uc_core::settings::model::Settings;

    /// 内存 SettingsPort，记录 save 调用次数以验证"同值不写盘"。
    #[derive(Default)]
    struct InMemorySettings {
        current: Mutex<Option<Settings>>,
        save_calls: Mutex<u32>,
    }

    #[async_trait]
    impl SettingsPort for InMemorySettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self
                .current
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(Settings::default))
        }
        async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
            *self.save_calls.lock().unwrap() += 1;
            *self.current.lock().unwrap() = Some(settings.clone());
            Ok(())
        }
    }

    fn build_uc(settings: Arc<InMemorySettings>) -> UpdateMobileSyncSettingsUseCase {
        UpdateMobileSyncSettingsUseCase::new(settings)
    }

    #[tokio::test]
    async fn enabling_from_default_writes_and_flags_restart() {
        let settings = Arc::new(InMemorySettings::default());
        let uc = build_uc(settings.clone());

        let out = uc
            .execute(UpdateMobileSyncSettingsInput { enabled: true })
            .await
            .expect("ok");
        assert!(out.enabled);
        assert!(out.restart_required);
        assert_eq!(*settings.save_calls.lock().unwrap(), 1);
        assert!(
            settings
                .current
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .mobile_sync
                .enabled
        );
    }

    #[tokio::test]
    async fn disabling_from_enabled_writes_and_flags_restart() {
        let settings = Arc::new(InMemorySettings::default());
        // 先把状态置为 enabled=true。
        let mut s = Settings::default();
        s.mobile_sync.enabled = true;
        settings.save(&s).await.unwrap();
        let initial_saves = *settings.save_calls.lock().unwrap();

        let uc = build_uc(settings.clone());
        let out = uc
            .execute(UpdateMobileSyncSettingsInput { enabled: false })
            .await
            .expect("ok");
        assert!(!out.enabled);
        assert!(out.restart_required);
        // 仅本次 use case 触发了一次新的 save。
        assert_eq!(*settings.save_calls.lock().unwrap(), initial_saves + 1);
    }

    #[tokio::test]
    async fn same_value_skips_save_and_clears_restart_required() {
        let settings = Arc::new(InMemorySettings::default()); // 默认 enabled=false
        let uc = build_uc(settings.clone());

        let out = uc
            .execute(UpdateMobileSyncSettingsInput { enabled: false })
            .await
            .expect("ok");
        assert!(!out.enabled);
        assert!(!out.restart_required);
        assert_eq!(
            *settings.save_calls.lock().unwrap(),
            0,
            "same value must not write"
        );
    }

    #[tokio::test]
    async fn translates_load_error() {
        struct FailingLoad;
        #[async_trait]
        impl SettingsPort for FailingLoad {
            async fn load(&self) -> anyhow::Result<Settings> {
                Err(anyhow::anyhow!("disk unreadable"))
            }
            async fn save(&self, _: &Settings) -> anyhow::Result<()> {
                unreachable!("load 失败时不应到 save")
            }
        }
        let uc = UpdateMobileSyncSettingsUseCase::new(Arc::new(FailingLoad));
        let err = uc
            .execute(UpdateMobileSyncSettingsInput { enabled: true })
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                UpdateMobileSyncSettingsError::SettingsLoadFailed(ref s) if s.contains("disk unreadable")
            ),
            "expected SettingsLoadFailed(disk unreadable), got {err:?}"
        );
    }

    #[tokio::test]
    async fn translates_save_error() {
        struct LoadOkSaveFail;
        #[async_trait]
        impl SettingsPort for LoadOkSaveFail {
            async fn load(&self) -> anyhow::Result<Settings> {
                Ok(Settings::default()) // enabled = false
            }
            async fn save(&self, _: &Settings) -> anyhow::Result<()> {
                Err(anyhow::anyhow!("disk full"))
            }
        }
        let uc = UpdateMobileSyncSettingsUseCase::new(Arc::new(LoadOkSaveFail));
        // 触发改动：enabled=true（默认是 false）。
        let err = uc
            .execute(UpdateMobileSyncSettingsInput { enabled: true })
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                UpdateMobileSyncSettingsError::SettingsSaveFailed(ref s) if s.contains("disk full")
            ),
            "expected SettingsSaveFailed(disk full), got {err:?}"
        );
    }
}
