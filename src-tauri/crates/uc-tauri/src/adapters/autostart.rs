use anyhow::Result;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt as _;
use uc_platform::ports::AutostartPort;

/// Tauri-specific runtime adapter for autostart functionality.
///
/// This adapter must only be constructed inside Tauri setup phase
/// and must not be used outside uc-tauri.
pub struct TauriAutostart {
    app_handle: AppHandle,
}

impl TauriAutostart {
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

/// Reconcile the OS-level launch-at-login registration with the desired state.
///
/// This is the single place that turns a stored `auto_start` preference into an
/// OS side effect, shared by the `update_autostart` command and the startup
/// reconciliation in `run.rs`.
///
/// When enabling, the registration is **always** (re)written so the launch
/// entry points at the current executable path. `auto-launch` records the exe
/// path at `enable()` time, so an entry left behind by an older install, a dev
/// build, or a moved binary keeps pointing at a stale (often deleted) path and
/// silently fails to launch. Rewriting on every enable self-heals that.
///
/// When disabling, the entry is removed only if it is currently present, so we
/// never surface a spurious error from removing a registration that isn't there.
pub(crate) fn reconcile_autostart(port: &dyn AutostartPort, desired: bool) -> Result<()> {
    if desired {
        port.enable()?;
        tracing::debug!("OS autostart registration (re)written for current executable");
    } else if port.is_enabled()? {
        port.disable()?;
        tracing::debug!("OS autostart registration removed");
    }
    Ok(())
}
