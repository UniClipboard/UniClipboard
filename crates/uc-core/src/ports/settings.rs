use async_trait::async_trait;

use crate::settings::model::Settings;

#[async_trait]
pub trait SettingsPort: Send + Sync {
    async fn load(&self) -> anyhow::Result<Settings>;
    async fn save(&self, settings: &Settings) -> anyhow::Result<()>;
}

pub trait SettingsMigrationPort: Send + Sync {
    fn from_version(&self) -> u32;
    fn to_version(&self) -> u32;
    fn migrate(&self, settings: Settings) -> Settings;
}
