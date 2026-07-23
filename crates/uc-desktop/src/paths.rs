use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DesktopPaths {
    pub settings_path: PathBuf,
    pub logs_dir: PathBuf,
    pub app_data_root_dir: PathBuf,
}

impl DesktopPaths {
    pub fn resolve() -> anyhow::Result<Self> {
        let app_data_root_dir = uc_app_paths::app_data_root()
            .ok_or_else(|| anyhow::anyhow!("unable to resolve app data root directory"))?;
        let logs_dir = uc_app_paths::app_log_dir()
            .ok_or_else(|| anyhow::anyhow!("unable to resolve app log directory"))?;

        Ok(Self {
            settings_path: app_data_root_dir.join("settings.json"),
            logs_dir,
            app_data_root_dir,
        })
    }

    pub fn last_notified_update_path(&self) -> PathBuf {
        self.app_data_root_dir.join("last_notified_update.json")
    }

    pub fn skipped_version_path(&self) -> PathBuf {
        self.app_data_root_dir.join("skipped_version.json")
    }

    pub fn update_prompt_throttle_path(&self) -> PathBuf {
        self.app_data_root_dir.join("update_prompt_throttle.json")
    }
}
