//! `GetMobileSyncSettingsUseCase` —— 给 UI / CLI 读取移动端同步功能的当前
//! 状态。
//!
//! 这是一个纯读 use case，对持久化设置（`Settings.mobile_sync.enabled`）
//! 与运行时状态（当前 LAN endpoint）做一次拼装；它不修改任何东西。
//!
//! 设计要点：
//!
//! 1. **持久化字段刻意只有 `enabled`**：v1 SPEC 决定 `listen_port`、
//!    `bind_address`、`require_signing` 等都不暴露给用户配置（要么由
//!    daemon 端常量推导，要么强制 ON）。如果我们把这些做成"读"接口的字
//!    段，将来想去掉时会成 breaking change，所以一开始就不放进 view。
//!
//! 2. **`current_lan_url` 由 endpoint port 实时探测**：daemon 没启动 LAN
//!    监听就拿不到 URL —— 让 view 显式区分 `enabled = true` 且 url
//!    `Some` 才算"真正在监听"，这样前端能据此提示"请重启 daemon"。
//!
//! 3. **`shortcut_install_methods` 在 application 层枚举**：哪个安装路径可
//!    用是产品策略而非领域真相，`uc-core` 不该承担这种"v1 / v2 切换"逻
//!    辑。它住在这里，将来开放 IcloudGeneric 时改一行常量即可。

use std::sync::Arc;

use tracing::instrument;

use uc_core::ports::{EndpointInfoError, MobileSyncEndpointInfoPort, SettingsPort};

// ─── public-shaped (output / error) ─────────────────────────────────────

/// UI / CLI 拿到的"当前移动端同步状态"快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileSyncSettingsView {
    /// 持久化 `Settings.mobile_sync.enabled`(总开关)。
    pub enabled: bool,
    /// 持久化 `Settings.mobile_sync.lan_listen_enabled`(LAN listener 子开关)。
    pub lan_listen_enabled: bool,
    /// 持久化 `Settings.mobile_sync.lan_bind_ip`。`None` 时 daemon 取
    /// `127.0.0.1` 作为兜底。
    pub lan_bind_ip: Option<String>,
    /// 持久化 `Settings.mobile_sync.lan_port`。`None` 时 daemon 取默认 42720。
    pub lan_port: Option<u16>,
    /// 当前 daemon 实际监听的 LAN URL(`http://<ip>:<port>`)。
    /// `None` 通常意味着"`enabled` 或 `lan_listen_enabled` 刚被设为 true
    /// 但 daemon 还没重启"或者"网卡列表里没有可用的 RFC1918 地址"。
    pub current_lan_url: Option<String>,
    /// `.shortcut` 的可选安装方式列表,按 SPEC §13 的产品策略定义可
    /// 用与不可用项;UI 据此渲染 disabled 选项与提示文案。
    pub shortcut_install_methods: Vec<ShortcutInstallMethodOption>,
}

/// `.shortcut` 的可选安装方式（应用层视图，不入 core）。
///
/// `TokenInjected` —— 路径 A：服务端动态打包带 token 的 `.shortcut`，
/// iPhone Safari 直接下载安装。v1 唯一可用。
///
/// `IcloudGeneric` —— 路径 B：用户从 iCloud 下载通用模板后粘贴
/// `uniclip://config?u=...&t=...`。v2 才会启用，目前 UI 显示为禁用项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutInstallMethod {
    TokenInjected,
    IcloudGeneric,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutInstallMethodOption {
    pub method: ShortcutInstallMethod,
    pub available: bool,
    /// 当 `available = false` 时给用户看的人话原因；`available = true`
    /// 时为 `None`。
    pub disabled_reason: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum GetMobileSyncSettingsError {
    #[error("settings load failed: {0}")]
    SettingsLoadFailed(String),

    #[error("endpoint info probe failed: {0}")]
    EndpointInfoFailed(String),
}

// ─── use case ───────────────────────────────────────────────────────────

pub(crate) struct GetMobileSyncSettingsUseCase {
    settings: Arc<dyn SettingsPort>,
    endpoint_info: Arc<dyn MobileSyncEndpointInfoPort>,
}

impl GetMobileSyncSettingsUseCase {
    pub(crate) fn new(
        settings: Arc<dyn SettingsPort>,
        endpoint_info: Arc<dyn MobileSyncEndpointInfoPort>,
    ) -> Self {
        Self {
            settings,
            endpoint_info,
        }
    }

    #[instrument(skip(self))]
    pub(crate) async fn execute(
        &self,
    ) -> Result<MobileSyncSettingsView, GetMobileSyncSettingsError> {
        let settings = self
            .settings
            .load()
            .await
            .map_err(|err| GetMobileSyncSettingsError::SettingsLoadFailed(err.to_string()))?;
        let mobile = settings.mobile_sync.clone();

        let endpoint = self
            .endpoint_info
            .current_lan_endpoint()
            .await
            .map_err(translate_endpoint_error)?;
        let current_lan_url = endpoint.map(|e| e.url);

        Ok(MobileSyncSettingsView {
            enabled: mobile.enabled,
            lan_listen_enabled: mobile.lan_listen_enabled,
            lan_bind_ip: mobile.lan_bind_ip,
            lan_port: mobile.lan_port,
            current_lan_url,
            shortcut_install_methods: shortcut_install_methods_v1(),
        })
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

/// v1 的可用安装方式集合。
///
/// 单独抽函数是为了让"开放 IcloudGeneric"成为单点改动 —— 改这里 + 把
/// SPEC §13 的对应描述更新即可。
fn shortcut_install_methods_v1() -> Vec<ShortcutInstallMethodOption> {
    vec![
        ShortcutInstallMethodOption {
            method: ShortcutInstallMethod::TokenInjected,
            available: true,
            disabled_reason: None,
        },
        ShortcutInstallMethodOption {
            method: ShortcutInstallMethod::IcloudGeneric,
            available: false,
            disabled_reason: Some("v1 暂不支持，将在后续版本启用".into()),
        },
    ]
}

fn translate_endpoint_error(err: EndpointInfoError) -> GetMobileSyncSettingsError {
    match err {
        EndpointInfoError::Storage(msg) => GetMobileSyncSettingsError::EndpointInfoFailed(msg),
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;

    use uc_core::mobile_sync::LanEndpointInfo;
    use uc_core::settings::model::Settings;

    /// 内存化的 SettingsPort：每次 `load` 返回最近一次 `save` 的副本，
    /// 没保存过则返回 `Settings::default()`。
    #[derive(Default)]
    struct InMemorySettings {
        current: Mutex<Option<Settings>>,
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
            *self.current.lock().unwrap() = Some(settings.clone());
            Ok(())
        }
    }

    struct FixedEndpoint(Option<&'static str>);
    #[async_trait]
    impl MobileSyncEndpointInfoPort for FixedEndpoint {
        async fn current_lan_endpoint(&self) -> Result<Option<LanEndpointInfo>, EndpointInfoError> {
            Ok(self.0.map(|url| LanEndpointInfo { url: url.into() }))
        }
    }

    fn build_uc(
        settings: Arc<InMemorySettings>,
        endpoint: Option<&'static str>,
    ) -> GetMobileSyncSettingsUseCase {
        GetMobileSyncSettingsUseCase::new(settings, Arc::new(FixedEndpoint(endpoint)))
    }

    #[tokio::test]
    async fn defaults_disabled_with_no_endpoint() {
        let settings = Arc::new(InMemorySettings::default());
        let uc = build_uc(settings, None);
        let v = uc.execute().await.expect("ok");
        assert!(!v.enabled);
        assert!(v.current_lan_url.is_none());
        // v1 install_methods：A 可用、B 不可用（顺序固定）。
        assert_eq!(v.shortcut_install_methods.len(), 2);
        assert!(matches!(
            v.shortcut_install_methods[0],
            ShortcutInstallMethodOption {
                method: ShortcutInstallMethod::TokenInjected,
                available: true,
                ..
            }
        ));
        assert!(matches!(
            v.shortcut_install_methods[1],
            ShortcutInstallMethodOption {
                method: ShortcutInstallMethod::IcloudGeneric,
                available: false,
                ..
            }
        ));
        assert!(v.shortcut_install_methods[1]
            .disabled_reason
            .as_deref()
            .unwrap()
            .contains("v1"));
    }

    #[tokio::test]
    async fn reads_enabled_from_persisted_settings() {
        let settings_port = Arc::new(InMemorySettings::default());
        let mut s = Settings::default();
        s.mobile_sync.enabled = true;
        settings_port.save(&s).await.unwrap();

        let uc = build_uc(settings_port, Some("http://192.168.1.5:42720"));
        let v = uc.execute().await.expect("ok");
        assert!(v.enabled);
        assert_eq!(
            v.current_lan_url.as_deref(),
            Some("http://192.168.1.5:42720")
        );
    }

    #[tokio::test]
    async fn enabled_true_but_no_endpoint_yet() {
        // 用户刚改 enabled，daemon 还没重启 —— view 应清楚地把"开关 ON
        // 但还没有 URL"两个状态分开返回。
        let settings_port = Arc::new(InMemorySettings::default());
        let mut s = Settings::default();
        s.mobile_sync.enabled = true;
        settings_port.save(&s).await.unwrap();

        let uc = build_uc(settings_port, None);
        let v = uc.execute().await.expect("ok");
        assert!(v.enabled);
        assert!(v.current_lan_url.is_none());
    }

    #[tokio::test]
    async fn translates_endpoint_storage_error() {
        struct ExplodingEndpoint;
        #[async_trait]
        impl MobileSyncEndpointInfoPort for ExplodingEndpoint {
            async fn current_lan_endpoint(
                &self,
            ) -> Result<Option<LanEndpointInfo>, EndpointInfoError> {
                Err(EndpointInfoError::Storage("ifaddr lookup failed".into()))
            }
        }
        let uc = GetMobileSyncSettingsUseCase::new(
            Arc::new(InMemorySettings::default()),
            Arc::new(ExplodingEndpoint),
        );
        let err = uc.execute().await.unwrap_err();
        assert!(
            matches!(err, GetMobileSyncSettingsError::EndpointInfoFailed(ref s)
                if s.contains("ifaddr lookup failed")),
            "expected EndpointInfoFailed(ifaddr lookup failed), got {err:?}"
        );
    }
}
