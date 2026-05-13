use std::sync::Arc;

use tracing::instrument;

use uc_core::ports::SettingsPort;

use crate::facade::settings::models::{
    apply_settings_patch, validate_settings, SettingsPatch, SettingsView,
};

#[derive(Debug, thiserror::Error)]
pub enum SettingsFacadeError {
    #[error("failed to load settings: {0}")]
    Load(String),
    #[error("failed to save settings: {0}")]
    Save(String),
    #[error("invalid settings: {0}")]
    Invalid(String),
}

pub struct SettingsFacade {
    settings: Arc<dyn SettingsPort>,
}

impl SettingsFacade {
    pub fn new(settings: Arc<dyn SettingsPort>) -> Self {
        Self { settings }
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
