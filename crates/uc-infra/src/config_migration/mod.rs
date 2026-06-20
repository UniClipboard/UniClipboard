//! Whole-installation configuration migration: `.ucbundle` codec, secrets
//! enumeration, db snapshot, and the staging contract.
//!
//! This module implements the `uc-core` config-migration ports
//! (`ExportConfigBundlePort` / `PreviewConfigImportPort` /
//! `StageConfigImportPort`) with a single [`ConfigMigrationAdapter`]. It owns
//! the bundle's persistence format (header + AEAD + tar + manifest) and the
//! on-disk staging contract a later restart applies.
//!
//! Submodule responsibilities:
//!
//! * [`bundle`] — `.ucbundle` header + Argon2id-keyed XChaCha20-Poly1305 seal.
//! * [`archive`] — uncompressed tar pack/unpack with path-safety + size bounds.
//! * [`manifest`] — inner `manifest.json` schema + version.
//! * [`secret_keys`] — centralized list of secure-storage entries to migrate.
//! * [`db_snapshot`] — consistent sqlite snapshot via `VACUUM INTO`.
//! * [`staging`] — `import-staging/` layout + `pending-import.json` marker +
//!   `secrets.json` format (the boot-time apply contract).
//! * [`adapter`] — the port-implementing adapter that ties them together.

pub mod adapter;
pub mod archive;
pub mod bundle;
pub mod db_snapshot;
pub mod manifest;
pub mod secret_keys;
pub mod staging;

pub use adapter::{ConfigMigrationAdapter, ConfigMigrationPaths};

#[cfg(test)]
mod tests {
    //! End-to-end adapter tests: export → preview → stage against real ports.

    use std::sync::Arc;

    use uc_core::crypto::domain::Passphrase;
    use uc_core::ids::ProfileId;
    use uc_core::ports::config_migration::{
        ConfigMigrationError, ExportConfigBundlePort, PreviewConfigImportPort,
        StageConfigImportPort,
    };
    use uc_core::ports::{ClockPort, LocalIdentityPort, SecureStorageError, SecureStoragePort};
    use uc_core::security::IdentityFingerprint;

    use super::staging::{
        PendingImportMarker, SecretsFile, StagingLayout, DEVICE_ID_MEMBER, KEYSLOT_MEMBER,
    };
    use super::{ConfigMigrationAdapter, ConfigMigrationPaths};
    use crate::db::pool::init_db_pool;

    use std::collections::HashMap;
    use std::sync::Mutex;

    type DbPool =
        diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<diesel::sqlite::SqliteConnection>>;

    #[derive(Default)]
    struct InMemorySecureStorage {
        map: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecureStoragePort for InMemorySecureStorage {
        fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(self.map.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
            self.map
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_vec());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
            self.map.lock().unwrap().remove(key);
            Ok(())
        }
    }

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    struct FixedIdentity(IdentityFingerprint);
    #[async_trait::async_trait]
    impl LocalIdentityPort for FixedIdentity {
        async fn create(&self) -> Result<IdentityFingerprint, uc_core::ports::LocalIdentityError> {
            Ok(self.0.clone())
        }
        async fn ensure(&self) -> Result<IdentityFingerprint, uc_core::ports::LocalIdentityError> {
            Ok(self.0.clone())
        }
        async fn get_current_fingerprint(
            &self,
        ) -> Result<Option<IdentityFingerprint>, uc_core::ports::LocalIdentityError> {
            Ok(Some(self.0.clone()))
        }
    }

    struct Fixture {
        adapter: Arc<ConfigMigrationAdapter>,
        secure_storage: Arc<InMemorySecureStorage>,
        _dir: tempfile::TempDir,
        export_dir: std::path::PathBuf,
        data_root: std::path::PathBuf,
    }

    fn build_fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let data_root = dir.path().join("data");
        let vault = data_root.join("vault");
        std::fs::create_dir_all(&vault).unwrap();

        // Real db pool (creates uniclipboard.db + runs migrations).
        let db_path = data_root.join("uniclipboard.db");
        let pool: DbPool = init_db_pool(db_path.to_str().unwrap()).unwrap();

        // Seed vault files + settings.
        std::fs::write(vault.join("keyslot.json"), b"{\"version\":\"V1\"}").unwrap();
        std::fs::write(
            vault.join("device_id.txt"),
            b"550e8400-e29b-41d4-a716-446655440000",
        )
        .unwrap();
        std::fs::write(data_root.join("settings.json"), b"{\"schema_version\":1}").unwrap();

        // Seed secrets: device identity (32 bytes) + current-profile KEK.
        let secure_storage = Arc::new(InMemorySecureStorage::default());
        secure_storage.set("iroh-identity:v1", &[7u8; 32]).unwrap();
        secure_storage
            .set("kek:v1:profile:default", &[9u8; 32])
            .unwrap();

        let identity = IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap();

        let paths = ConfigMigrationPaths {
            db_path,
            vault_dir: vault,
            settings_path: data_root.join("settings.json"),
            app_data_root: data_root.clone(),
        };

        let adapter = Arc::new(ConfigMigrationAdapter::new(
            secure_storage.clone(),
            pool,
            Arc::new(FixedIdentity(identity)),
            Arc::new(FixedClock(1_700_000_000_000)),
            paths,
            ProfileId::from("default".to_string()),
        ));

        let export_dir = dir.path().join("exports");
        std::fs::create_dir_all(&export_dir).unwrap();

        Fixture {
            adapter,
            secure_storage,
            _dir: dir,
            export_dir,
            data_root,
        }
    }

    #[tokio::test]
    async fn export_then_preview_round_trips_manifest() {
        let fx = build_fixture();
        let password = Passphrase::from("space-passphrase");
        let dest = fx.export_dir.join("config.ucbundle");

        let written = fx
            .adapter
            .export_bundle(&password, &dest)
            .await
            .expect("export should succeed");
        assert_eq!(written, dest);
        assert!(dest.exists());

        let preview = fx
            .adapter
            .preview_import(&password, &dest)
            .await
            .expect("preview should succeed");

        assert_eq!(preview.created_at_unix_ms, 1_700_000_000_000);
        assert_eq!(preview.profile_id, ProfileId::from("default".to_string()));
        assert_eq!(preview.device_fingerprint, "ABCD-EFGH-IJKL-MNOP");
        assert!(!preview.app_version.is_empty());
    }

    #[tokio::test]
    async fn preview_with_wrong_password_is_invalid_or_corrupt() {
        let fx = build_fixture();
        let dest = fx.export_dir.join("config.ucbundle");
        fx.adapter
            .export_bundle(&Passphrase::from("right"), &dest)
            .await
            .unwrap();

        let err = fx
            .adapter
            .preview_import(&Passphrase::from("wrong"), &dest)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ConfigMigrationError::InvalidPasswordOrCorrupt
        ));
    }

    #[tokio::test]
    async fn stage_lays_out_staging_with_kek_and_no_unlock_required() {
        let fx = build_fixture();
        let password = Passphrase::from("pw");
        let dest = fx.export_dir.join("config.ucbundle");
        fx.adapter.export_bundle(&password, &dest).await.unwrap();

        let staged = fx
            .adapter
            .stage_import(&password, &dest)
            .await
            .expect("stage should succeed");

        // KEK was present in storage, so applying needs no further unlock.
        assert!(!staged.unlock_required_after_apply);

        let layout = StagingLayout::new(&fx.data_root);
        assert!(layout.marker_path().exists());

        let marker: PendingImportMarker =
            serde_json::from_slice(&std::fs::read(layout.marker_path()).unwrap()).unwrap();
        assert!(marker.has_kek);

        // Staged secrets carry both the identity and the KEK (base64).
        let secrets_path = layout.staging_dir().join("secrets.json");
        let secrets: SecretsFile =
            serde_json::from_slice(&std::fs::read(secrets_path).unwrap()).unwrap();
        assert!(secrets.secrets.contains_key("iroh-identity:v1"));
        assert!(secrets.secrets.contains_key("kek:v1:profile:default"));

        // Vault + db members landed in staging.
        assert!(layout.staging_dir().join(KEYSLOT_MEMBER).exists());
        assert!(layout.staging_dir().join(DEVICE_ID_MEMBER).exists());
        assert!(layout.staging_dir().join("db/uniclipboard.db").exists());
    }

    #[tokio::test]
    async fn stage_without_kek_requires_unlock_after_apply() {
        let fx = build_fixture();
        // Drop the KEK before export so the bundle carries identity only.
        fx.secure_storage.delete("kek:v1:profile:default").unwrap();

        let password = Passphrase::from("pw");
        let dest = fx.export_dir.join("config.ucbundle");
        fx.adapter.export_bundle(&password, &dest).await.unwrap();

        let staged = fx.adapter.stage_import(&password, &dest).await.unwrap();
        assert!(staged.unlock_required_after_apply);

        let layout = StagingLayout::new(&fx.data_root);
        let marker: PendingImportMarker =
            serde_json::from_slice(&std::fs::read(layout.marker_path()).unwrap()).unwrap();
        assert!(!marker.has_kek);
    }

    #[tokio::test]
    async fn export_fails_when_device_identity_secret_missing() {
        let fx = build_fixture();
        // Remove the required identity secret.
        fx.secure_storage.delete("iroh-identity:v1").unwrap();

        let err = fx
            .adapter
            .export_bundle(&Passphrase::from("pw"), &fx.export_dir.join("x.ucbundle"))
            .await
            .unwrap_err();
        assert!(matches!(err, ConfigMigrationError::Internal { .. }));
    }
}
