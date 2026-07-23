//! Desktop secure-storage preparation and legacy identity migration.

use std::path::PathBuf;
use std::sync::Arc;

use uc_platform::file_secure_storage::FileSecureStorage;
use uc_platform::migrating_secure_storage::MigratingSecureStorage;
use uc_platform::ports::SecureStorageProvider;

use super::error::{WiringError, WiringResult};
use crate::layer::paths::{apply_profile_suffix, DesktopHostPaths};

const LEGACY_IDENTITY_STORE_KEY: &str = "iroh-identity:v1";

pub(crate) struct SecureStoragePrelude {
    pub(crate) secure_storage: Arc<dyn SecureStorageProvider>,
}

pub(crate) fn build_identity_storage(
    primary: Arc<dyn SecureStorageProvider>,
    legacy_identity_dir: PathBuf,
) -> Arc<dyn SecureStorageProvider> {
    let legacy: Arc<dyn SecureStorageProvider> =
        Arc::new(FileSecureStorage::with_base_dir(legacy_identity_dir));
    Arc::new(MigratingSecureStorage::new(
        primary,
        legacy,
        vec![LEGACY_IDENTITY_STORE_KEY.to_string()],
    ))
}

pub(crate) fn build_secure_storage_prelude(
    paths: &DesktopHostPaths,
) -> WiringResult<SecureStoragePrelude> {
    let app_data_root = paths.app_data_root_dir.clone();
    let secure_storage =
        uc_platform::secure_storage::create_default_secure_storage_in_app_data_root(
            app_data_root.clone(),
        )
        .map_err(|error| WiringError::SecureStorageInit(error.to_string()))?;

    let default_legacy_identity_dir = app_data_root.join("iroh-identity");
    let profiled_legacy_identity_dir = apply_profile_suffix(default_legacy_identity_dir.clone());
    std::fs::create_dir_all(&profiled_legacy_identity_dir).map_err(|error| {
        WiringError::SecureStorageInit(format!(
            "failed to create legacy iroh-identity directory: {error}"
        ))
    })?;

    let secure_storage = build_identity_storage(secure_storage, profiled_legacy_identity_dir);
    let secure_storage = if default_legacy_identity_dir
        == apply_profile_suffix(default_legacy_identity_dir.clone())
    {
        secure_storage
    } else {
        build_identity_storage(secure_storage, default_legacy_identity_dir)
    };

    Ok(SecureStoragePrelude { secure_storage })
}
