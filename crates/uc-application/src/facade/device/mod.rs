use std::sync::Arc;

use tracing::instrument;

use uc_core::ports::{DeviceIdentityPort, SettingsPort};

const DEFAULT_DEVICE_NAME: &str = "Uniclipboard Device";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDeviceInfoView {
    pub peer_id: String,
    pub device_name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceFacadeError {
    #[error("failed to read local device identity: {0}")]
    DeviceIdentity(String),
}

pub struct DeviceFacade {
    device_identity: Arc<dyn DeviceIdentityPort>,
    settings: Arc<dyn SettingsPort>,
}

impl DeviceFacade {
    pub fn new(
        device_identity: Arc<dyn DeviceIdentityPort>,
        settings: Arc<dyn SettingsPort>,
    ) -> Self {
        Self {
            device_identity,
            settings,
        }
    }

    #[instrument(skip_all)]
    pub async fn local_device_info(&self) -> Result<LocalDeviceInfoView, DeviceFacadeError> {
        let device_name = match self.settings.load().await {
            Ok(settings) => normalize_device_name(settings.general.device_name),
            Err(err) => {
                tracing::warn!(error = %err, "device facade: settings load failed; using fallback device name");
                DEFAULT_DEVICE_NAME.to_string()
            }
        };

        Ok(LocalDeviceInfoView {
            peer_id: self.device_identity.current_device_id().to_string(),
            device_name,
        })
    }
}

fn normalize_device_name(value: Option<String>) -> String {
    let name = value.unwrap_or_default();
    let trimmed = name.trim();
    if trimmed.is_empty() {
        DEFAULT_DEVICE_NAME.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use std::sync::Mutex;
    use uc_core::ids::DeviceId;
    use uc_core::ports::DeviceIdentityPort;
    use uc_core::settings::model::Settings;

    struct StaticDeviceIdentity;

    impl DeviceIdentityPort for StaticDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            DeviceId::new("dev-1")
        }
    }

    struct InMemorySettings {
        settings: Mutex<Settings>,
        fail_load: bool,
    }

    #[async_trait]
    impl SettingsPort for InMemorySettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            if self.fail_load {
                anyhow::bail!("settings unavailable");
            }
            Ok(self.settings.lock().unwrap().clone())
        }

        async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
            *self.settings.lock().unwrap() = settings.clone();
            Ok(())
        }
    }

    fn facade_with(device_name: Option<String>, fail_load: bool) -> DeviceFacade {
        let mut settings = Settings::default();
        settings.general.device_name = device_name;
        DeviceFacade::new(
            Arc::new(StaticDeviceIdentity),
            Arc::new(InMemorySettings {
                settings: Mutex::new(settings),
                fail_load,
            }),
        )
    }

    #[tokio::test]
    async fn local_device_info_returns_trimmed_settings_name() {
        let info = facade_with(Some("  MacBook  ".to_string()), false)
            .local_device_info()
            .await
            .expect("ok");

        assert_eq!(info.peer_id, "dev-1");
        assert_eq!(info.device_name, "MacBook");
    }

    #[tokio::test]
    async fn local_device_info_uses_fallback_when_name_blank_or_settings_fail() {
        let blank = facade_with(Some("  ".to_string()), false)
            .local_device_info()
            .await
            .expect("blank ok");
        let failed = facade_with(Some("Ignored".to_string()), true)
            .local_device_info()
            .await
            .expect("failed settings still ok");

        assert_eq!(blank.device_name, DEFAULT_DEVICE_NAME);
        assert_eq!(failed.device_name, DEFAULT_DEVICE_NAME);
    }
}
