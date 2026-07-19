//! Desktop host preparation for the shared core wiring.
//!
//! This module owns desktop directory discovery, secure-storage setup, legacy
//! identity import, system clipboard selection, and host observer assembly.
//! The shared wiring module only receives already prepared inputs.

use std::path::PathBuf;
use std::sync::Arc;

use uc_application::facade::{AppPaths, HostEventEmitterPort};
use uc_core::config::AppConfig;
use uc_core::ports::SecureStoragePort;

use crate::layer::paths::{apply_profile_suffix, get_default_app_dirs, resolve_app_paths};
use crate::layer::platform::create_desktop_system_clipboard;
use crate::wiring::deps::{BackgroundRuntimeDeps, WiredDependencies, WiringError, WiringResult};
use crate::wiring::wire::{wire_dependencies_from_inputs, CoreWiringInputs};

struct SecureStoragePrelude {
    secure_storage: Arc<dyn SecureStoragePort>,
    legacy_iroh_identity_dir: PathBuf,
}

/// Prepare secure storage and apply any pending desktop configuration import
/// before the database is opened.
fn build_secure_storage_prelude(paths: &AppPaths) -> WiringResult<SecureStoragePrelude> {
    let app_data_root = paths.app_data_root_dir.clone();

    let secure_storage =
        uc_platform::secure_storage::create_default_secure_storage_in_app_data_root(
            app_data_root.clone(),
        )
        .map_err(|e| WiringError::SecureStorageInit(e.to_string()))?;

    // Old backups contain iroh identity files. Restore them to this migration
    // source, then let the shared identity-storage wrapper move the identity
    // into secure storage on first access.
    let legacy_iroh_identity_dir = apply_profile_suffix(app_data_root.join("iroh-identity"));
    std::fs::create_dir_all(&legacy_iroh_identity_dir).map_err(|e| {
        WiringError::SecureStorageInit(format!(
            "failed to create legacy iroh-identity dir {}: {e}",
            legacy_iroh_identity_dir.display()
        ))
    })?;

    crate::startup::pending_import::apply_pending_import(
        &app_data_root,
        &paths.db_path,
        &paths.vault_dir,
        &paths.settings_path,
        &legacy_iroh_identity_dir,
        &secure_storage,
    )
    .map_err(|e| WiringError::PendingImport(e.to_string()))?;

    Ok(SecureStoragePrelude {
        secure_storage,
        legacy_iroh_identity_dir,
    })
}

/// Prepare desktop host capabilities and assemble the process runtime.
pub fn wire_dependencies(
    config: &AppConfig,
) -> WiringResult<(WiredDependencies, BackgroundRuntimeDeps)> {
    let platform_dirs = get_default_app_dirs()?;
    let paths = resolve_app_paths(&platform_dirs, config)?;
    let SecureStoragePrelude {
        secure_storage,
        legacy_iroh_identity_dir,
    } = build_secure_storage_prelude(&paths)?;

    let system_clipboard = create_desktop_system_clipboard()?;
    let analytics_sink = crate::subsystem::analytics::build_analytics_sink();
    let analytics_facade = crate::subsystem::analytics::build_analytics_facade(
        &analytics_sink,
        &paths.app_data_root_dir,
    );
    let host_event_emitter = Arc::new(crate::observability::host_event::LoggingHostEventEmitter)
        as Arc<dyn HostEventEmitterPort>;
    let iroh_blob_store_dir = apply_profile_suffix(paths.app_data_root_dir.join("iroh-blobs"));

    wire_dependencies_from_inputs(CoreWiringInputs {
        paths,
        secure_storage,
        legacy_iroh_identity_dir,
        iroh_blob_store_dir,
        system_clipboard,
        analytics_sink,
        analytics_facade,
        host_event_emitter,
    })
}
