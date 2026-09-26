//! Desktop secure-storage preparation.
//!
//! The host store only exposes entries the platform keystore actually holds.
//! `<app_data_root>/iroh-identity` belongs to the Engine, which reads and
//! writes the network identity there directly; it must never be aliased into
//! this store, or the Engine's migration cleanup of host entries would delete
//! the live identity.

use std::sync::Arc;

use uc_platform::ports::SecureStorageProvider;

use super::error::{WiringError, WiringResult};
use crate::layer::paths::DesktopHostPaths;

pub(crate) struct SecureStoragePrelude {
    pub(crate) secure_storage: Arc<dyn SecureStorageProvider>,
}

pub(crate) fn build_secure_storage_prelude(
    paths: &DesktopHostPaths,
) -> WiringResult<SecureStoragePrelude> {
    let secure_storage =
        uc_platform::secure_storage::create_default_secure_storage_in_app_data_root(
            paths.app_data_root_dir.clone(),
        )
        .map_err(|error| WiringError::SecureStorageInit(error.to_string()))?;

    Ok(SecureStoragePrelude { secure_storage })
}

#[cfg(test)]
mod tests {
    use uc_platform::file_secure_storage::FileSecureStorage;
    use uc_platform::ports::SecureStorageProvider;

    const ENGINE_IDENTITY_KEY: &str = "iroh-identity:v1";

    #[test]
    fn host_store_cleanup_never_touches_the_engine_identity_directory() {
        let temporary = tempfile::tempdir().unwrap();
        let app_data_root = temporary.path().join("app.uniclipboard.desktop-a");
        let engine_identity = FileSecureStorage::with_base_dir(app_data_root.join("iroh-identity"));
        engine_identity
            .set(ENGINE_IDENTITY_KEY, b"engine-owned")
            .unwrap();
        let host = FileSecureStorage::new_in_app_data_root(app_data_root).unwrap();

        assert_eq!(host.get(ENGINE_IDENTITY_KEY).unwrap(), None);
        host.delete(ENGINE_IDENTITY_KEY).unwrap();

        assert_eq!(
            engine_identity.get(ENGINE_IDENTITY_KEY).unwrap().as_deref(),
            Some(&b"engine-owned"[..])
        );
    }

    #[test]
    fn a_real_legacy_host_entry_remains_visible_for_engine_migration() {
        let temporary = tempfile::tempdir().unwrap();
        let host = FileSecureStorage::new_in_app_data_root(temporary.path().to_path_buf()).unwrap();
        host.set(ENGINE_IDENTITY_KEY, b"legacy-host-entry").unwrap();

        assert_eq!(
            host.get(ENGINE_IDENTITY_KEY).unwrap().as_deref(),
            Some(&b"legacy-host-entry"[..])
        );
        host.delete(ENGINE_IDENTITY_KEY).unwrap();
        assert_eq!(host.get(ENGINE_IDENTITY_KEY).unwrap(), None);
    }
}
