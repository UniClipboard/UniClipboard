use std::collections::HashMap;
use std::io::ErrorKind;

use serde::Deserialize;
use uc_daemon_contract::api::dto::settings::{
    QuickPanelDoubleTapModifierDto, QuickPanelPositionDto, ShortcutKeyDto,
};

use crate::runtime::DesktopRuntime;

pub struct DesktopShellSettings {
    pub silent_start: bool,
    pub is_silent_mode: bool,
    pub language: String,
    pub lan_only_active: bool,
    pub quick_panel_enabled: bool,
    pub quick_panel_position: QuickPanelPositionDto,
    pub quick_panel_double_tap_modifier: QuickPanelDoubleTapModifierDto,
    pub auto_start: bool,
    pub keyboard_shortcuts: HashMap<String, ShortcutKeyDto>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct StoredSettings {
    general: StoredGeneralSettings,
    network: StoredNetworkSettings,
    quick_panel: StoredQuickPanelSettings,
    keyboard_shortcuts: HashMap<String, ShortcutKeyDto>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct StoredGeneralSettings {
    auto_start: bool,
    startup_mode: StoredStartupMode,
    language: Option<String>,
}

#[derive(Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum StoredStartupMode {
    #[default]
    Normal,
    Silent,
    Lightweight,
}

#[derive(Deserialize)]
#[serde(default)]
struct StoredNetworkSettings {
    allow_relay_fallback: bool,
}

impl Default for StoredNetworkSettings {
    fn default() -> Self {
        Self {
            allow_relay_fallback: true,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct StoredQuickPanelSettings {
    enabled: bool,
    position: QuickPanelPositionDto,
    double_tap_modifier: QuickPanelDoubleTapModifierDto,
}

impl Default for StoredQuickPanelSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            position: QuickPanelPositionDto::Center,
            double_tap_modifier: QuickPanelDoubleTapModifierDto::Disabled,
        }
    }
}

impl DesktopRuntime {
    pub async fn load_shell_settings(&self) -> anyhow::Result<DesktopShellSettings> {
        let stored = match tokio::fs::read(&self.storage_paths().settings_path).await {
            Ok(bytes) => serde_json::from_slice::<StoredSettings>(&bytes)?,
            Err(error) if error.kind() == ErrorKind::NotFound => StoredSettings::default(),
            Err(error) => return Err(error.into()),
        };
        let is_silent_mode = stored.general.startup_mode == StoredStartupMode::Silent;
        let silent_start =
            is_silent_mode || stored.general.startup_mode == StoredStartupMode::Lightweight;

        Ok(DesktopShellSettings {
            silent_start,
            is_silent_mode,
            language: stored.general.language.unwrap_or_default(),
            lan_only_active: !stored.network.allow_relay_fallback,
            quick_panel_enabled: stored.quick_panel.enabled,
            quick_panel_position: stored.quick_panel.position,
            quick_panel_double_tap_modifier: stored.quick_panel.double_tap_modifier,
            auto_start: stored.general.auto_start,
            keyboard_shortcuts: stored.keyboard_shortcuts,
        })
    }

    pub async fn ensure_default_device_name(&self) -> anyhow::Result<()> {
        let path = &self.storage_paths().settings_path;
        let mut document = match tokio::fs::read(path).await {
            Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)?,
            Err(error) if error.kind() == ErrorKind::NotFound => serde_json::json!({}),
            Err(error) => return Err(error.into()),
        };
        let Some(root) = document.as_object_mut() else {
            anyhow::bail!("settings root must be a JSON object");
        };
        let general = root
            .entry("general")
            .or_insert_with(|| serde_json::json!({}));
        let Some(general) = general.as_object_mut() else {
            anyhow::bail!("settings.general must be a JSON object");
        };
        if general
            .get("device_name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|name| !name.is_empty())
        {
            return Ok(());
        }

        let hostname = gethostname::gethostname()
            .to_str()
            .unwrap_or("Uniclipboard Device")
            .to_string();
        general.insert(
            "device_name".to_string(),
            serde_json::Value::String(hostname),
        );

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let temporary = path.with_extension("json.tmp");
        tokio::fs::write(&temporary, serde_json::to_vec_pretty(&document)?).await?;
        #[cfg(windows)]
        if path.exists() {
            let _ = tokio::fs::remove_file(path).await;
        }
        tokio::fs::rename(temporary, path).await?;
        Ok(())
    }
}
