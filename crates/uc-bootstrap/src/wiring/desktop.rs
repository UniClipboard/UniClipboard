//! Desktop host preparation for the shared core wiring.
//!
//! This module owns desktop directory discovery, secure-storage setup, legacy
//! identity import, system clipboard selection, and host observer assembly.
//! The shared wiring module only receives already prepared inputs.

use std::path::PathBuf;
use std::sync::Arc;

use uc_application::facade::{AppPaths, HostEventEmitterPort};
use uc_core::config::AppConfig;
use uc_core::ids::ProfileId;
use uc_core::ports::SecureStoragePort;
use uc_infra::network::iroh::IDENTITY_STORE_KEY;
use uc_platform::file_secure_storage::FileSecureStorage;
use uc_platform::migrating_secure_storage::MigratingSecureStorage;

use crate::layer::paths::{apply_profile_suffix, get_default_app_dirs, resolve_app_paths};
use crate::layer::platform::create_desktop_system_clipboard;
use crate::wiring::deps::{BackgroundRuntimeDeps, WiredDependencies, WiringError, WiringResult};
use crate::wiring::wire::{wire_dependencies_from_inputs, CoreWiringInputs};

struct SecureStoragePrelude {
    secure_storage: Arc<dyn SecureStoragePort>,
    legacy_iroh_identity_dir: PathBuf,
}

pub(crate) fn build_identity_storage(
    primary: Arc<dyn SecureStoragePort>,
    legacy_identity_dir: PathBuf,
) -> Arc<dyn SecureStoragePort> {
    let legacy: Arc<dyn SecureStoragePort> =
        Arc::new(FileSecureStorage::with_base_dir(legacy_identity_dir));
    Arc::new(MigratingSecureStorage::new(
        primary,
        legacy,
        vec![IDENTITY_STORE_KEY.to_string()],
    ))
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
    let secure_storage = build_identity_storage(secure_storage, legacy_iroh_identity_dir.clone());

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
        profile_id: ProfileId::from("default"),
        legacy_iroh_identity_dir,
        iroh_blob_store_dir,
        system_clipboard,
        analytics_sink,
        analytics_facade,
        host_event_emitter,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fmt::Display;
    use std::sync::{Arc, Mutex};

    use uc_core::ports::{SecureStorageError, SecureStoragePort};

    use super::{build_identity_storage, FileSecureStorage, IDENTITY_STORE_KEY};

    #[derive(Default)]
    struct MemorySecureStorage {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    fn must<T, E: Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("test setup failed: {error}"),
        }
    }

    impl MemorySecureStorage {
        fn values(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<u8>>> {
            match self.values.lock() {
                Ok(values) => values,
                Err(poisoned) => poisoned.into_inner(),
            }
        }
    }

    impl SecureStoragePort for MemorySecureStorage {
        fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(self.values().get(key).cloned())
        }

        fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
            self.values().insert(key.to_owned(), value.to_vec());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
            self.values().remove(key);
            Ok(())
        }
    }

    #[test]
    fn legacy_file_identity_moves_into_primary_secure_storage() {
        let temp = must(tempfile::tempdir());
        let legacy_dir = temp.path().join("iroh-identity");
        must(std::fs::create_dir_all(&legacy_dir));
        let legacy = FileSecureStorage::with_base_dir(legacy_dir.clone());
        let identity = [7u8; 32];
        must(legacy.set(IDENTITY_STORE_KEY, &identity));
        let primary = Arc::new(MemorySecureStorage::default());

        let storage = build_identity_storage(primary.clone(), legacy_dir);
        let loaded = must(storage.get(IDENTITY_STORE_KEY));

        assert_eq!(loaded.as_deref(), Some(identity.as_slice()));
        assert_eq!(
            must(primary.get(IDENTITY_STORE_KEY)).as_deref(),
            Some(identity.as_slice())
        );
        assert!(must(legacy.get(IDENTITY_STORE_KEY)).is_none());
    }
}
