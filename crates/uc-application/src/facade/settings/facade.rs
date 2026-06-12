use std::sync::Arc;

use tracing::instrument;

use uc_core::ports::SettingsPort;

use crate::facade::settings::models::{
    apply_settings_patch, validate_settings, SettingsPatch, SettingsView,
};
use crate::facade::settings::relay_diagnostic::{
    RelayDiagnosticPort, RelayProbeError, RelayProbeReport,
};

#[derive(Debug, thiserror::Error)]
pub enum SettingsFacadeError {
    #[error("failed to load settings: {0}")]
    Load(String),
    #[error("failed to save settings: {0}")]
    Save(String),
    #[error("invalid settings: {0}")]
    Invalid(String),
    /// Relay 探测能力未在本进程装配。常见于 webserver / 单元测试场景。
    #[error("relay probe is unavailable in this runtime")]
    RelayProbeUnavailable,
    #[error("invalid relay URL: {0}")]
    RelayProbeInvalidUrl(String),
    #[error("dns lookup failed: {0}")]
    RelayProbeDns(String),
    #[error("tls handshake failed: {0}")]
    RelayProbeTls(String),
    #[error("relay handshake failed: {0}")]
    RelayProbeHandshake(String),
    #[error("relay probe timed out")]
    RelayProbeTimeout,
    #[error("relay probe failed: {0}")]
    RelayProbeOther(String),
}

/// 应用层暴露的中继探测结果视图。沿用核心层的字段语义,但与 core 类型解耦,
/// 上层(daemon / tauri / cli)只需要消费此类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayProbeReportView {
    pub latency_ms: u32,
}

impl From<RelayProbeReport> for RelayProbeReportView {
    fn from(value: RelayProbeReport) -> Self {
        Self {
            latency_ms: value.latency_ms,
        }
    }
}

impl From<RelayProbeError> for SettingsFacadeError {
    fn from(value: RelayProbeError) -> Self {
        match value {
            RelayProbeError::InvalidUrl(msg) => SettingsFacadeError::RelayProbeInvalidUrl(msg),
            RelayProbeError::Dns(msg) => SettingsFacadeError::RelayProbeDns(msg),
            RelayProbeError::Tls(msg) => SettingsFacadeError::RelayProbeTls(msg),
            RelayProbeError::Handshake(msg) => SettingsFacadeError::RelayProbeHandshake(msg),
            RelayProbeError::Timeout => SettingsFacadeError::RelayProbeTimeout,
            RelayProbeError::Other(msg) => SettingsFacadeError::RelayProbeOther(msg),
        }
    }
}

pub struct SettingsFacade {
    settings: Arc<dyn SettingsPort>,
    relay_diagnostic: Option<Arc<dyn RelayDiagnosticPort>>,
}

impl SettingsFacade {
    pub fn new(settings: Arc<dyn SettingsPort>) -> Self {
        Self {
            settings,
            relay_diagnostic: None,
        }
    }

    /// 注入中继诊断端口。Production daemon 会通过 bootstrap 调用,
    /// webserver / 单元测试可以不装配,此时 [`Self::probe_relay_url`]
    /// 会返回 [`SettingsFacadeError::RelayProbeUnavailable`]。
    pub fn with_relay_diagnostic(mut self, port: Arc<dyn RelayDiagnosticPort>) -> Self {
        self.relay_diagnostic = Some(port);
        self
    }

    /// 对一个候选中继 URL 发起一次可达性探测。
    ///
    /// 不读取也不修改任何已持久化的设置,允许重复调用。失败时把领域错误
    /// 翻译到 [`SettingsFacadeError`] 的细分变体,便于上层做有针对性的
    /// 用户提示。
    #[instrument(skip(self), fields(relay_url = %url))]
    pub async fn probe_relay_url(
        &self,
        url: &str,
    ) -> Result<RelayProbeReportView, SettingsFacadeError> {
        let port = self
            .relay_diagnostic
            .as_ref()
            .ok_or(SettingsFacadeError::RelayProbeUnavailable)?;
        let report = port.probe(url).await?;
        Ok(report.into())
    }

    #[instrument(skip_all)]
    pub async fn get(&self) -> Result<SettingsView, SettingsFacadeError> {
        self.settings
            .load()
            .await
            .map(SettingsView::from)
            .map_err(|err| SettingsFacadeError::Load(err.to_string()))
    }

    #[instrument(skip_all)]
    pub async fn update(&self, patch: SettingsPatch) -> Result<SettingsView, SettingsFacadeError> {
        let existing = self
            .settings
            .load()
            .await
            .map_err(|err| SettingsFacadeError::Load(err.to_string()))?;
        let merged = apply_settings_patch(existing, patch);
        validate_settings(&merged).map_err(SettingsFacadeError::Invalid)?;
        self.settings
            .save(&merged)
            .await
            .map_err(|err| SettingsFacadeError::Save(err.to_string()))?;
        Ok(merged.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use std::sync::Mutex;
    use uc_core::settings::model::Settings;

    struct InMemorySettings {
        settings: Mutex<Settings>,
        fail_save: bool,
    }

    #[async_trait]
    impl SettingsPort for InMemorySettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self.settings.lock().unwrap().clone())
        }

        async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
            if self.fail_save {
                anyhow::bail!("disk full");
            }
            *self.settings.lock().unwrap() = settings.clone();
            Ok(())
        }
    }

    fn facade_with(settings: Settings) -> SettingsFacade {
        SettingsFacade::new(Arc::new(InMemorySettings {
            settings: Mutex::new(settings),
            fail_save: false,
        }))
    }

    #[tokio::test]
    async fn update_merges_general_and_sync_patch_without_exposing_core_model() {
        let mut seed = Settings::default();
        seed.general.device_name = Some("old".to_string());
        seed.sync.content_types.image = true;

        let facade = facade_with(seed);
        let view = facade
            .update(SettingsPatch {
                general: Some(crate::facade::settings::GeneralSettingsPatch {
                    device_name: Some(Some("new".to_string())),
                    ..Default::default()
                }),
                sync: Some(crate::facade::settings::SyncSettingsPatch {
                    content_types: Some(crate::facade::settings::ContentTypesPatch {
                        text: Some(false),
                        image: None,
                        link: None,
                        file: None,
                        code_snippet: None,
                        rich_text: None,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .await
            .expect("settings update ok");

        assert_eq!(view.general.device_name.as_deref(), Some("new"));
        assert!(!view.sync.content_types.text);
        assert!(view.sync.content_types.image);
    }

    #[tokio::test]
    async fn update_surfaces_save_failure() {
        let facade = SettingsFacade::new(Arc::new(InMemorySettings {
            settings: Mutex::new(Settings::default()),
            fail_save: true,
        }));

        let err = facade.update(SettingsPatch::default()).await.unwrap_err();
        assert!(matches!(err, SettingsFacadeError::Save(_)));
    }

    #[tokio::test]
    async fn update_rejects_invalid_custom_relay_url() {
        let facade = facade_with(Settings::default());
        let err = facade
            .update(SettingsPatch {
                network: Some(crate::facade::settings::NetworkSettingsPatch {
                    custom_relay_urls: Some(vec!["ftp://relay.example.com".to_string()]),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .await
            .unwrap_err();

        assert!(matches!(err, SettingsFacadeError::Invalid(_)));
    }
}
