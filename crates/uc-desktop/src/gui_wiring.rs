//! GUI client wiring — assembles the file-backed ports a pure-client GUI needs.
//!
//! This replaces the `uc_bootstrap::build_gui_client_context()` call for the
//! `uc-desktop` crate, removing the dependency on `uc-bootstrap` (and by
//! extension, uc-infra / iroh / diesel / sqlite).
//!
//! The GUI client is a pure client of the external `uniclipd` daemon (ADR-008
//! P3-3 B2'-3).  It never opens the sqlite pool — the daemon owns all
//! sqlite-backed state.  This module assembles only the file-backed / in-memory
//! ports the GUI process needs:
//!
//! - [`SettingsPort`] — read/write `settings.json`
//! - [`SetupStatusPort`] — read/write `.setup_status` in the vault dir
//! - [`AnalyticsPort`] — Noop sink (the daemon is the authoritative sender)
//! - Device identity — load or create `device_id.txt`
//! - [`AppPaths`] — resolved storage paths

use std::sync::Arc;

use uc_core::app_dirs::{AppDirs, AppPaths};
use uc_core::ports::{DeviceIdentityPort, SettingsPort, SetupStatusPort};
use uc_observability::analytics::AnalyticsPort;

/// Ensures the device has a valid name by initializing it with the system hostname if empty.
///
/// When the application starts, this function checks if `device_name` is `None` or an empty
/// string. If so, it fetches the system hostname and saves it as the default device name.
///
/// This is a GUI startup utility — the daemon handles its own device name initialization
/// independently.
pub async fn ensure_default_device_name(
    settings: Arc<dyn SettingsPort>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut current_settings = settings.load().await?;

    let needs_initialization = current_settings.general.device_name.is_none()
        || current_settings.general.device_name.as_deref() == Some("");

    if needs_initialization {
        let hostname = gethostname::gethostname()
            .to_str()
            .unwrap_or("Uniclipboard Device")
            .to_string();

        tracing::info!("Initializing default device name: {}", hostname);

        current_settings.general.device_name = Some(hostname);
        settings.save(&current_settings).await?;
    }

    Ok(())
}

use crate::file_ports::{FileSettingsRepository, FileSetupStatusRepository, LocalDeviceIdentity};

/// File-backed / in-memory ports a pure-client GUI needs (ADR-008 P3-3 B2'-3).
///
/// The external `uniclipd` daemon owns the sqlite pool, blob store, clipboard
/// infra, and the in-process `AppFacade`; a GUI that is a pure client of that
/// daemon must never open the same database (split-brain), so it assembles only
/// this subset.
pub struct GuiClientDeps {
    pub settings: Arc<dyn SettingsPort>,
    pub setup_status: Arc<dyn SetupStatusPort>,
    pub analytics: Arc<dyn AnalyticsPort>,
    pub device_id: String,
    pub storage_paths: AppPaths,
}

/// Assemble only the file-backed / in-memory ports a pure-client GUI needs,
/// WITHOUT opening the sqlite pool, initialising secure storage, or building
/// any blob / clipboard infra.
///
/// All four ports here are file-backed or in-memory: settings (`settings.json`),
/// setup-status (vault dir file), the device identity (app-data-root identity
/// dir) and the analytics sink (Noop — the daemon is the single authoritative
/// analytics sender).  They are the same files the daemon reads, so the GUI
/// reading them concurrently is eventually-consistent and split-brain-free.
pub fn build_gui_client_context() -> anyhow::Result<GuiClientDeps> {
    // Resolve application directories using the lightweight uc-app-paths crate.
    let app_data_root = uc_app_paths::app_data_root()
        .ok_or_else(|| anyhow::anyhow!("unable to resolve app data root directory"))?;
    let app_cache_root = uc_app_paths::app_cache_root()
        .ok_or_else(|| anyhow::anyhow!("unable to resolve app cache root directory"))?;

    let dirs = AppDirs {
        app_data_root,
        app_cache_root,
    };
    let paths = AppPaths::from_app_dirs(&dirs);

    let settings: Arc<dyn SettingsPort> =
        Arc::new(FileSettingsRepository::new(paths.settings_path.clone()));

    let setup_status: Arc<dyn SetupStatusPort> = Arc::new(
        FileSetupStatusRepository::with_defaults(paths.vault_dir.clone()),
    );

    // ADR-008 D20: a pure-client GUI is NOT an analytics sender — the daemon is
    // the single authoritative PostHog sender.  So the GUI client gets a Noop
    // sink here; the GUI shell replaces this with a daemon-forwarding sink once
    // it has the daemon connection state (see `uc_tauri::run`).
    let analytics: Arc<dyn AnalyticsPort> =
        Arc::new(uc_observability::analytics::NoopAnalyticsSink);

    let device_identity: Arc<dyn DeviceIdentityPort> = Arc::new(
        LocalDeviceIdentity::load_or_create(paths.app_data_root_dir.clone())
            .map_err(|e| anyhow::anyhow!("Failed to create device identity: {e}"))?,
    );
    let device_id = device_identity.current_device_id().to_string();

    Ok(GuiClientDeps {
        settings,
        setup_status,
        analytics,
        device_id,
        storage_paths: paths,
    })
}
