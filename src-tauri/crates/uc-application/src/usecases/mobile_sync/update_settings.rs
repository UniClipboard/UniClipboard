//! # 为什么需要这个 use case
//!
//! 持久化用户对"移动端同步"四个字段(enabled / lan_listen_enabled /
//! lan_advertise_ip / lan_port)的修改 —— 单点把 patch 翻译成"落盘 Settings
//! 的具体 mutation",让 facade 层不直接接触 [`SettingsPort`] 的 load/save
//! 节奏与原子性细节。
//!
//! # 输出语义
//!
//! 返回值 `restart_required` 是个 **wire-兼容历史字段** —— 在 lifecycle
//! port 引入前(2026-05-11 之前),它告诉调用方"这次改动要等下一次 daemon
//! 重启才生效"。现在装入 [`MobileLanLifecyclePort`] 的 facade 路径会在
//! 写盘后立即即时生效 + 把这个字段拍回 false,前端 UI 不再弹"请重启"。
//! 没装 lifecycle port 的装配(CLI fallback / 单测)仍按"有变化 → true /
//! 同值 → false"的旧语义返回,保留向后兼容。
//!
//! # 实现细节
//!
//! 1. **load → mutate → save 的原子性是 SettingsPort 适配器的职责**。本
//!    use case 不持锁也不重读校验:现有 `SettingsPort` 实现是单写者
//!    (daemon 进程独占), 并发竞态非问题。如果将来引入 multi-writer, 需要
//!    在 SettingsPort 层提供 CAS 或事务能力,而不是在 use case 里"再 load
//!    一遍"做乐观比较 —— 那只会被认为安全实则有 ABA。
//!
//! 2. **`restart_required` 的判定一律基于"有效变更"**。即任一字段实际
//!    发生变化才置 `true`; 同值不写盘且返回 `false`,避免幂等操作触发
//!    无意义的 restart 提示。
//!
//! 3. **没有 `dry_run` 选项**:设置项简单, UI 直接保存即可,无需先预演。

use std::sync::Arc;

use tracing::instrument;

use uc_core::ports::SettingsPort;

// ─── public-shaped (input / output / error) ─────────────────────────────

/// 更新移动端同步设置的 patch 输入。
///
/// 每个字段都是 `Option<...>`:
/// * `None` —— 该字段保持不变;
/// * `Some(value)` —— 把该字段写入 `value`(可能与现状相同 → 不写盘)。
///
/// 这样让 CLI / 前端都能以"只改自己关心的字段"的方式调用,无需先 read-
/// modify-write。`lan_advertise_ip` 用嵌套 `Option<Option<String>>` 表达三态:
/// `None` = 不动、`Some(None)` = 显式清空、`Some(Some(ip))` = 写入。
#[derive(Debug, Clone, Default)]
pub struct UpdateMobileSyncSettingsInput {
    pub enabled: Option<bool>,
    pub lan_listen_enabled: Option<bool>,
    pub lan_advertise_ip: Option<Option<String>>,
    pub lan_port: Option<Option<u16>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateMobileSyncSettingsOutput {
    /// 落盘后的 `enabled` 值。
    pub enabled: bool,
    /// 落盘后的 `lan_listen_enabled` 值。
    pub lan_listen_enabled: bool,
    /// 落盘后的 `lan_advertise_ip` 值。
    pub lan_advertise_ip: Option<String>,
    /// 落盘后的 `lan_port` 值。
    pub lan_port: Option<u16>,
    /// Wire-兼容历史字段。在 lifecycle port 引入前,这个标志告诉调用方
    /// "这次改动要等下一次 daemon 重启才生效"; 引入后,装入
    /// [`uc_core::ports::MobileLanLifecyclePort`] 的 facade 路径会即时
    /// 生效并把此字段拍回 `false`。无 lifecycle 装配(CLI fallback / 单测)
    /// 保留旧语义:任一字段实际变化 → `true`,同值不写盘 → `false`。
    pub restart_required: bool,
    /// 即时生效路径下 bind 失败的原因。
    ///
    /// 装入 lifecycle port 的 facade 路径会在写盘成功后调
    /// `lifecycle.apply(target)`;若 adapter 把新端口绑失败(端口占用 /
    /// 权限 / IP 不可分配等),facade 从 `MobileSyncEndpointInfoPort` 读出
    /// `BindFailed{reason}` 并把 reason 透传到此字段。
    ///
    /// 字段语义:
    /// - `Some(reason)` —— 落盘成功但 listener 没起来,UI 应当告知用户
    ///   原因并阻断后续依赖 listener 的动作(典型:首次添加移动设备)。
    /// - `None` —— 要么 lifecycle 没装(use case 自身永远填 None,由
    ///   facade 路径覆写),要么 bind 成功 / 目标本就是 Disabled。
    pub lan_listener_bind_error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateMobileSyncSettingsError {
    #[error("settings load failed: {0}")]
    SettingsLoadFailed(String),

    #[error("settings save failed: {0}")]
    SettingsSaveFailed(String),

    /// `lan_advertise_ip` 不是合法 IPv4 字面量 / `lan_port=0`。
    #[error("invalid LAN listener parameter: {0}")]
    InvalidLanParameter(String),
}

// ─── use case ───────────────────────────────────────────────────────────

pub(crate) struct UpdateMobileSyncSettingsUseCase {
    settings: Arc<dyn SettingsPort>,
}

impl UpdateMobileSyncSettingsUseCase {
    pub(crate) fn new(settings: Arc<dyn SettingsPort>) -> Self {
        Self { settings }
    }

    #[instrument(skip(self, input))]
    pub(crate) async fn execute(
        &self,
        input: UpdateMobileSyncSettingsInput,
    ) -> Result<UpdateMobileSyncSettingsOutput, UpdateMobileSyncSettingsError> {
        // 0. patch 字段的轻量校验 —— 在 load 前先把明显错误挡掉,避免无意义
        //    的 read-modify-write。
        if let Some(Some(ref ip_str)) = input.lan_advertise_ip {
            if ip_str.parse::<std::net::Ipv4Addr>().is_err() {
                return Err(UpdateMobileSyncSettingsError::InvalidLanParameter(format!(
                    "lan_advertise_ip is not a valid IPv4 address: {ip_str}"
                )));
            }
        }
        if let Some(Some(0)) = input.lan_port {
            return Err(UpdateMobileSyncSettingsError::InvalidLanParameter(
                "lan_port must be 1..=65535, got 0".into(),
            ));
        }

        let mut current =
            self.settings.load().await.map_err(|err| {
                UpdateMobileSyncSettingsError::SettingsLoadFailed(err.to_string())
            })?;

        // 1. 计算每个字段的"目标值",并一字段一字段对比是否变化。restart_required
        //    在任一字段实际变化时置 true。
        let prev = current.mobile_sync.clone();
        let target_enabled = input.enabled.unwrap_or(prev.enabled);
        let target_lan_listen_enabled = input.lan_listen_enabled.unwrap_or(prev.lan_listen_enabled);
        let target_lan_advertise_ip = input
            .lan_advertise_ip
            .clone()
            .unwrap_or_else(|| prev.lan_advertise_ip.clone());
        let target_lan_port = input.lan_port.unwrap_or(prev.lan_port);

        let restart_required = target_enabled != prev.enabled
            || target_lan_listen_enabled != prev.lan_listen_enabled
            || target_lan_advertise_ip != prev.lan_advertise_ip
            || target_lan_port != prev.lan_port;

        if restart_required {
            current.mobile_sync.enabled = target_enabled;
            current.mobile_sync.lan_listen_enabled = target_lan_listen_enabled;
            current.mobile_sync.lan_advertise_ip = target_lan_advertise_ip.clone();
            current.mobile_sync.lan_port = target_lan_port;
            self.settings.save(&current).await.map_err(|err| {
                UpdateMobileSyncSettingsError::SettingsSaveFailed(err.to_string())
            })?;
        }
        // 同值时跳过 save —— 避免 mtime / 文件系统副作用,也避免上层 watcher
        // 收到无意义的 settings-changed 事件。

        Ok(UpdateMobileSyncSettingsOutput {
            enabled: target_enabled,
            lan_listen_enabled: target_lan_listen_enabled,
            lan_advertise_ip: target_lan_advertise_ip,
            lan_port: target_lan_port,
            restart_required,
            // use case 不知道 lifecycle 是否被装配,更不知道 apply 后的
            // 端口状态。这个字段由 facade 路径在调完 lifecycle.apply 后
            // 从 MobileSyncEndpointInfoPort 读出来覆写。
            lan_listener_bind_error: None,
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
            .execute(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                ..Default::default()
            })
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
            .execute(UpdateMobileSyncSettingsInput {
                enabled: Some(false),
                ..Default::default()
            })
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
            .execute(UpdateMobileSyncSettingsInput {
                enabled: Some(false),
                ..Default::default()
            })
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
            .execute(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                ..Default::default()
            })
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
            .execute(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                ..Default::default()
            })
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

    #[tokio::test]
    async fn lan_fields_round_trip_through_patch() {
        let settings = Arc::new(InMemorySettings::default());
        let uc = build_uc(settings.clone());

        // 先把 lan 子字段全部写一轮:lan_listen_enabled / lan_advertise_ip /
        // lan_port 全都从 None / false 跳到具体值, restart_required 必为 true。
        let out = uc
            .execute(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                lan_listen_enabled: Some(true),
                lan_advertise_ip: Some(Some("192.168.1.5".into())),
                lan_port: Some(Some(42721)),
            })
            .await
            .expect("ok");
        assert!(out.enabled);
        assert!(out.lan_listen_enabled);
        assert_eq!(out.lan_advertise_ip.as_deref(), Some("192.168.1.5"));
        assert_eq!(out.lan_port, Some(42721));
        assert!(out.restart_required);

        // 部分字段 patch: 只清 lan_advertise_ip, 其它保持。
        let out2 = uc
            .execute(UpdateMobileSyncSettingsInput {
                lan_advertise_ip: Some(None),
                ..Default::default()
            })
            .await
            .expect("ok");
        assert!(
            out2.lan_listen_enabled,
            "lan_listen_enabled must be retained"
        );
        assert_eq!(
            out2.lan_advertise_ip, None,
            "lan_advertise_ip must be cleared"
        );
        assert_eq!(out2.lan_port, Some(42721), "lan_port must be retained");
        assert!(out2.restart_required);
    }

    #[tokio::test]
    async fn rejects_invalid_ipv4_string() {
        let settings = Arc::new(InMemorySettings::default());
        let uc = build_uc(settings);
        let err = uc
            .execute(UpdateMobileSyncSettingsInput {
                lan_advertise_ip: Some(Some("not-an-ip".into())),
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            UpdateMobileSyncSettingsError::InvalidLanParameter(ref s) if s.contains("not-an-ip")
        ));
    }

    #[tokio::test]
    async fn rejects_zero_port() {
        let settings = Arc::new(InMemorySettings::default());
        let uc = build_uc(settings);
        let err = uc
            .execute(UpdateMobileSyncSettingsInput {
                lan_port: Some(Some(0)),
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            UpdateMobileSyncSettingsError::InvalidLanParameter(ref s) if s.contains("must be 1..=65535")
        ));
    }
}
