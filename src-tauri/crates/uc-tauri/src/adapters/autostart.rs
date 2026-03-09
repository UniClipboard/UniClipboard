use anyhow::Result;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt as _;
use uc_core::ports::AutostartPort;

/// Tauri-specific runtime adapter for autostart functionality.
///
/// This adapter must only be constructed inside Tauri setup phase
/// and must not be used outside uc-tauri.
pub struct TauriAutostart {
    app_handle: AppHandle,
}

impl TauriAutostart {
    #[allow(dead_code)]
    pub(crate) fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

impl AutostartPort for TauriAutostart {
    fn is_enabled(&self) -> Result<bool> {
        self.app_handle
            .autolaunch()
            .is_enabled()
            .map_err(anyhow::Error::from)
    }

    fn enable(&self) -> Result<()> {
        self.app_handle.autolaunch().enable()?;
        Ok(())
    }

    fn disable(&self) -> Result<()> {
        self.app_handle.autolaunch().disable()?;
        Ok(())
    }
}
