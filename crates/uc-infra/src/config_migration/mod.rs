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
    //!
    //! The source is a *genuinely initialized* installation — a real `KeySlot`
    //! and matching KEK produced by `DefaultSpaceAccessAdapter::initialize`. The
    //! export seals the bundle with that KEK (no export password), so opening it
    //! requires the space passphrase that derives the KEK ([`FIXTURE_PASSPHRASE`]).

    use std::sync::Arc;

    use uc_core::crypto::domain::Passphrase;
    use uc_core::ids::{ProfileId, SpaceId};
    use uc_core::ports::config_migration::{
        ConfigMigrationError, ExportConfigBundlePort, PreviewConfigImportPort,
        StageConfigImportPort,
    };
    use uc_core::ports::space::SpaceAccessStore;
    use uc_core::ports::{ClockPort, LocalIdentityPort, SecureStorageError, SecureStoragePort};
    use uc_core::security::IdentityFingerprint;

    use super::staging::{
        PendingImportMarker, SecretsFile, StagingLayout, DEVICE_ID_MEMBER, IROH_IDENTITY_PREFIX,
        KEYSLOT_MEMBER, SETUP_STATUS_MEMBER,
    };
    use super::{ConfigMigrationAdapter, ConfigMigrationPaths};
    use crate::db::pool::init_db_pool;
    use crate::fs::key_slot_store::JsonKeySlotStore;
    use crate::security::{
        DefaultCurrentProfile, DefaultSpaceAccessAdapter, InMemorySession, KeyMaterialStore,
    };

    use std::collections::HashMap;
    use std::sync::Mutex;

    type DbPool =
        diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<diesel::sqlite::SqliteConnection>>;

    /// Space passphrase used to initialize the fixture. The KEK is derived from
    /// it, so this same passphrase opens the exported bundle.
    const FIXTURE_PASSPHRASE: &str = "space-passphrase";

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

    async fn build_fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let data_root = dir.path().join("data");
        let vault = data_root.join("vault");
        std::fs::create_dir_all(&vault).unwrap();

        // Real db pool (creates uniclipboard.db + runs migrations).
        let db_path = data_root.join("uniclipboard.db");
        let pool: DbPool = init_db_pool(db_path.to_str().unwrap()).unwrap();

        // Real initialization: writes a real `vault/keyslot.json` and stores a
        // matching KEK (derived from FIXTURE_PASSPHRASE) in the backend. This is
        // what makes "open the bundle with the passphrase" hold.
        let secure_storage = Arc::new(InMemorySecureStorage::default());
        {
            let key_material = Arc::new(KeyMaterialStore::new(
                secure_storage.clone(),
                Arc::new(JsonKeySlotStore::new(vault.clone())),
            ));
            let space_access = DefaultSpaceAccessAdapter::new(
                key_material,
                Arc::new(DefaultCurrentProfile::new()),
                Arc::new(InMemorySession::new()),
            );
            SpaceAccessStore::initialize(
                &space_access,
                &SpaceId::from("space"),
                &Passphrase::from(FIXTURE_PASSPHRASE),
            )
            .await
            .unwrap();
        }

        // Remaining vault files + settings (carried verbatim).
        std::fs::write(
            vault.join("device_id.txt"),
            b"550e8400-e29b-41d4-a716-446655440000",
        )
        .unwrap();
        std::fs::write(
            vault.join(".setup_status"),
            b"{\"has_completed\":true,\"space_id\":null}",
        )
        .unwrap();
        std::fs::write(data_root.join("settings.json"), b"{\"schema_version\":1}").unwrap();

        // Seed the iroh device-identity directory with the 0600 files the
        // FileSecureStorage backend persists (migrated as files, not a secret).
        let iroh_identity_dir = data_root.join("iroh-identity");
        std::fs::create_dir_all(&iroh_identity_dir).unwrap();
        std::fs::write(iroh_identity_dir.join("iroh-identity_v1.bin"), [7u8; 32]).unwrap();

        let identity = IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap();

        let paths = ConfigMigrationPaths {
            db_path,
            vault_dir: vault,
            iroh_identity_dir,
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
        let fx = build_fixture().await;
        let password = Passphrase::from(FIXTURE_PASSPHRASE);
        let dest = fx.export_dir.join("config.ucbundle");

        let written = fx
            .adapter
            .export_bundle(&dest)
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
        let fx = build_fixture().await;
        let dest = fx.export_dir.join("config.ucbundle");
        fx.adapter.export_bundle(&dest).await.unwrap();

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
        let fx = build_fixture().await;
        let password = Passphrase::from(FIXTURE_PASSPHRASE);
        let dest = fx.export_dir.join("config.ucbundle");
        fx.adapter.export_bundle(&dest).await.unwrap();

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

        // Staged secrets carry ONLY the KEK (base64); the iroh identity is no
        // longer a secret — it migrates as files.
        let secrets_path = layout.staging_dir().join("secrets.json");
        let secrets: SecretsFile =
            serde_json::from_slice(&std::fs::read(secrets_path).unwrap()).unwrap();
        assert!(secrets.secrets.contains_key("kek:v1:profile:default"));
        assert!(!secrets.secrets.contains_key("iroh-identity:v1"));
        assert_eq!(secrets.secrets.len(), 1);

        // Vault + db members landed in staging.
        assert!(layout.staging_dir().join(KEYSLOT_MEMBER).exists());
        assert!(layout.staging_dir().join(DEVICE_ID_MEMBER).exists());
        assert!(layout.staging_dir().join("db/uniclipboard.db").exists());

        // The iroh device-identity files travel through the bundle into staging
        // under the identity prefix, so a later apply restores the same network
        // identity (no re-pairing).
        let staged_identity = layout
            .staging_dir()
            .join(format!("{IROH_IDENTITY_PREFIX}iroh-identity_v1.bin"));
        assert!(staged_identity.exists());
        assert_eq!(std::fs::read(staged_identity).unwrap(), vec![7u8; 32]);

        // The setup-status marker travels through the bundle into staging so a
        // later apply keeps the installation flagged as initialized.
        let staged_setup_status = layout.staging_dir().join(SETUP_STATUS_MEMBER);
        assert!(staged_setup_status.exists());
        assert_eq!(
            std::fs::read(staged_setup_status).unwrap(),
            b"{\"has_completed\":true,\"space_id\":null}"
        );
    }

    #[tokio::test]
    async fn export_without_kek_in_storage_fails() {
        // The KEK both seals the bundle and rides inside it; without it in
        // storage there is nothing to seal with (the passphrase is never
        // retained, so it cannot be re-derived here). Export must fail rather
        // than emit a bundle no passphrase could open.
        let fx = build_fixture().await;
        fx.secure_storage.delete("kek:v1:profile:default").unwrap();

        let dest = fx.export_dir.join("config.ucbundle");
        let err = fx
            .adapter
            .export_bundle(&dest)
            .await
            .expect_err("export must fail without a KEK to seal with");
        assert!(matches!(err, ConfigMigrationError::Internal { .. }));
        assert!(!dest.exists(), "no bundle should be written on failure");
    }

    #[tokio::test]
    async fn export_succeeds_when_iroh_identity_dir_absent() {
        let fx = build_fixture().await;
        // Remove the identity directory entirely: the export must still succeed
        // (the directory is carried defensively, not required), producing a
        // bundle with no iroh-identity members.
        std::fs::remove_dir_all(fx.data_root.join("iroh-identity")).unwrap();

        let password = Passphrase::from(FIXTURE_PASSPHRASE);
        let dest = fx.export_dir.join("config.ucbundle");
        fx.adapter
            .export_bundle(&dest)
            .await
            .expect("export should succeed without identity files");

        fx.adapter.stage_import(&password, &dest).await.unwrap();
        let layout = StagingLayout::new(&fx.data_root);
        // No identity files reached staging.
        assert!(!layout
            .staging_dir()
            .join(format!("{IROH_IDENTITY_PREFIX}iroh-identity_v1.bin"))
            .exists());
    }
}
