//! `ConfigMigrationAdapter` — single adapter implementing the three
//! configuration-migration ports (export / preview / stage).
//!
//! The three intents share packaging, key derivation, and the secrets/key
//! enumeration, so they live behind one adapter (the consumer wires it once and
//! holds it as three port handles). The adapter is *mechanical*: it packs,
//! seals, decrypts, validates structure, and stages. It performs **no** business
//! gating — "is the session unlocked", "is the target initialized" are the
//! caller's preconditions; this layer only emits the structural errors
//! `InvalidPasswordOrCorrupt` / `IncompatibleBundle` / `Io` / `Internal`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tracing::{error, info, instrument, warn};

use uc_core::crypto::domain::Passphrase;
use uc_core::ids::ProfileId;
use uc_core::ports::config_migration::{
    ConfigImportPreview, ConfigMigrationError, ConfigSourceMode, ExportConfigBundlePort,
    PreviewConfigImportPort, StageConfigImportPort, StagedConfigImport,
};
use uc_core::ports::{ClockPort, LocalIdentityPort, SecureStoragePort};

use crate::security::crypto_model::KeySlotFile;

use super::archive::{ArchiveError, BundleArchive};
use super::bundle::{self, Argon2Params, BundleError};
use super::db_snapshot::{self, DbSnapshotError};
use super::manifest::{BundleManifest, ManifestSourceMode, MANIFEST_MEMBER, MANIFEST_SCHEMA_VER};
use super::secret_keys::{migratable_secret_keys, MigratableSecretKind, SECRETS_MEMBER};
use super::staging::{
    PendingImportMarker, SecretsFile, StagingError, StagingLayout, DB_MEMBER, DEVICE_ID_MEMBER,
    IROH_IDENTITY_PREFIX, KEYSLOT_MEMBER, PENDING_IMPORT_SCHEMA_VER, SETTINGS_MEMBER,
    SETUP_STATUS_MEMBER, STAGING_DIR_NAME, UI_STATE_PREFIX,
};

/// Raw secure-storage entries collected for a bundle, paired with the
/// current-profile KEK bytes when one was among them.
///
/// The KEK is surfaced separately (not just a presence flag) because the export
/// both carries it inside the bundle *and* uses it as the bundle's own AEAD key.
type CollectedSecrets = (Vec<(String, Vec<u8>)>, Option<Vec<u8>>);

/// Filesystem inputs the adapter reads/writes. Resolved by the caller from the
/// installation's path layout so this adapter never recomputes directory
/// policy.
#[derive(Debug, Clone)]
pub struct ConfigMigrationPaths {
    /// Live sqlite database path (used only to derive a scratch snapshot path
    /// alongside it; the snapshot itself comes from the pool).
    pub db_path: PathBuf,
    /// Vault directory holding `keyslot.json` and `device_id.txt`.
    pub vault_dir: PathBuf,
    /// Directory holding the iroh device-identity files
    /// (`<app_data>/iroh-identity[_<profile>]/`). Its contents are carried as
    /// bundle files (not a credential-store secret), since production wiring
    /// persists the identity to `0600` files there rather than the keychain.
    pub iroh_identity_dir: PathBuf,
    /// User settings file (`settings.json`).
    pub settings_path: PathBuf,
    /// Data root: staging + marker + optional UI-state files live here.
    pub app_data_root: PathBuf,
}

impl ConfigMigrationPaths {
    fn ui_state_paths(&self) -> Vec<(String, PathBuf)> {
        // Optional UI-state files carried best-effort. Names mirror the data
        // root's update-state files.
        vec![
            (
                "last_notified_update.json".to_string(),
                self.app_data_root.join("last_notified_update.json"),
            ),
            (
                "skipped_version.json".to_string(),
                self.app_data_root.join("skipped_version.json"),
            ),
        ]
    }

    /// Scratch path for the `VACUUM INTO` snapshot, kept beside the live db so
    /// it lands on the same filesystem (no cross-device rename surprises).
    fn snapshot_scratch_path(&self) -> PathBuf {
        let parent = self
            .db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.app_data_root.clone());
        parent.join("uniclipboard.export-snapshot.db")
    }
}

/// The config-migration adapter.
pub struct ConfigMigrationAdapter {
    secure_storage: Arc<dyn SecureStoragePort>,
    db_pool: diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<diesel::sqlite::SqliteConnection>>,
    local_identity: Arc<dyn LocalIdentityPort>,
    clock: Arc<dyn ClockPort>,
    paths: ConfigMigrationPaths,
    profile_id: ProfileId,
}

impl ConfigMigrationAdapter {
    /// Wire the adapter.
    ///
    /// * `secure_storage` — reads the secrets to carry (and the boot step later
    ///   writes them back); same backend the rest of the app uses.
    /// * `db_pool` — produces the consistent db snapshot via `VACUUM INTO`.
    /// * `local_identity` — supplies the device fingerprint recorded in the
    ///   manifest.
    /// * `clock` — supplies the manifest's `created_at` (and the staging
    ///   timestamp).
    /// * `paths` — resolved filesystem layout.
    /// * `profile_id` — current profile; selects which KEK is enumerated.
    pub fn new(
        secure_storage: Arc<dyn SecureStoragePort>,
        db_pool: diesel::r2d2::Pool<
            diesel::r2d2::ConnectionManager<diesel::sqlite::SqliteConnection>,
        >,
        local_identity: Arc<dyn LocalIdentityPort>,
        clock: Arc<dyn ClockPort>,
        paths: ConfigMigrationPaths,
        profile_id: ProfileId,
    ) -> Self {
        Self {
            secure_storage,
            db_pool,
            local_identity,
            clock,
            paths,
            profile_id,
        }
    }

    /// Read an optional file: `Ok(None)` when absent, `Err` only on a real IO
    /// failure (not "missing").
    fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, ConfigMigrationError> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(ConfigMigrationError::Io {
                details: "failed reading a configuration file".to_string(),
            }),
        }
    }

    /// Read a required file: missing or unreadable both fail (the caller's
    /// precondition is that an initialized installation has these).
    fn read_required(path: &Path) -> Result<Vec<u8>, ConfigMigrationError> {
        std::fs::read(path).map_err(|_| ConfigMigrationError::Internal {
            details: "a required configuration file was missing or unreadable".to_string(),
        })
    }

    /// Gather the secrets enumerated for the current profile from secure
    /// storage. Returns the raw entries plus the current-profile KEK bytes when
    /// one was present.
    ///
    /// Only the current-profile KEK is a credential-store secret here. It is
    /// read once and used twice by the export: as the bundle's own AEAD key and
    /// as a carried secret so the target can auto-unlock after apply. The iroh
    /// device identity is *not* read here; it migrates as files (see
    /// [`Self::collect_iroh_identity_files`]).
    fn collect_secrets(&self) -> Result<CollectedSecrets, ConfigMigrationError> {
        let mut entries = Vec::new();
        let mut kek = None;

        for spec in migratable_secret_keys(self.profile_id.inner()) {
            let value =
                self.secure_storage
                    .get(&spec.key)
                    .map_err(|_| ConfigMigrationError::Internal {
                        details: "secure storage read failed while collecting secrets".to_string(),
                    })?;

            if let Some(bytes) = value {
                if matches!(spec.kind, MigratableSecretKind::ProfileKek) {
                    kek = Some(bytes.clone());
                }
                entries.push((spec.key, bytes));
            }
        }

        Ok((entries, kek))
    }

    /// Collect the iroh device-identity files as bundle members under
    /// [`IROH_IDENTITY_PREFIX`].
    ///
    /// Defensive: an initialized source should have identity files, but a
    /// missing or empty directory is not fatal — it is skipped rather than
    /// failing the export. Only regular files at the directory's top level are
    /// carried (the `FileSecureStorage` backend keeps flat `*.bin` files).
    fn collect_iroh_identity_files(&self) -> Result<Vec<(String, Vec<u8>)>, ConfigMigrationError> {
        let dir = &self.paths.iroh_identity_dir;
        let read_dir = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => {
                return Err(ConfigMigrationError::Io {
                    details: "failed reading the device-identity directory".to_string(),
                })
            }
        };

        let mut files = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|_| ConfigMigrationError::Io {
                details: "failed enumerating the device-identity directory".to_string(),
            })?;
            let file_type = entry.file_type().map_err(|_| ConfigMigrationError::Io {
                details: "failed inspecting a device-identity entry".to_string(),
            })?;
            if !file_type.is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let bytes = Self::read_required(&entry.path())?;
            files.push((format!("{IROH_IDENTITY_PREFIX}{name}"), bytes));
        }
        Ok(files)
    }

    fn source_mode() -> ManifestSourceMode {
        if uc_app_paths::is_portable() {
            ManifestSourceMode::Portable
        } else {
            ManifestSourceMode::Installed
        }
    }

    /// Decrypt a bundle file and parse its inner archive + manifest.
    async fn open_archive(
        &self,
        password: &Passphrase,
        source: &Path,
    ) -> Result<(BundleArchive, BundleManifest), ConfigMigrationError> {
        let source = source.to_path_buf();
        let password_bytes = password.expose().to_string();

        // Read + decrypt + untar on a blocking thread: file IO + Argon2 are both
        // blocking and CPU-bound.
        let (archive, manifest) = tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(&source).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ConfigMigrationError::Io {
                        details: "bundle source file not found".to_string(),
                    }
                } else {
                    ConfigMigrationError::Io {
                        details: "failed reading bundle source".to_string(),
                    }
                }
            })?;

            let password = Passphrase::from(password_bytes);
            let tar_bytes = bundle::open(&password, &bytes).map_err(map_bundle_err)?;
            let archive = BundleArchive::from_tar_bytes(&tar_bytes).map_err(map_archive_err)?;

            let manifest_bytes = archive.get(MANIFEST_MEMBER).ok_or_else(|| {
                ConfigMigrationError::IncompatibleBundle {
                    reason: "bundle is missing its manifest".to_string(),
                }
            })?;
            let manifest: BundleManifest =
                serde_json::from_slice(manifest_bytes).map_err(|_| {
                    ConfigMigrationError::IncompatibleBundle {
                        reason: "bundle manifest could not be parsed".to_string(),
                    }
                })?;

            if manifest.schema_ver > MANIFEST_SCHEMA_VER {
                return Err(ConfigMigrationError::IncompatibleBundle {
                    reason: format!(
                        "bundle archive schema {} is newer than supported {}",
                        manifest.schema_ver, MANIFEST_SCHEMA_VER
                    ),
                });
            }

            Ok::<_, ConfigMigrationError>((archive, manifest))
        })
        .await
        .map_err(|_| ConfigMigrationError::Internal {
            details: "bundle decode task failed to run".to_string(),
        })??;

        Ok((archive, manifest))
    }
}

fn map_bundle_err(err: BundleError) -> ConfigMigrationError {
    match err {
        BundleError::InvalidOrCorrupt => ConfigMigrationError::InvalidPasswordOrCorrupt,
        BundleError::Incompatible(reason) => ConfigMigrationError::IncompatibleBundle { reason },
        BundleError::Crypto => ConfigMigrationError::Internal {
            details: "bundle cryptographic operation failed".to_string(),
        },
    }
}

fn map_archive_err(err: ArchiveError) -> ConfigMigrationError {
    match err {
        // A decrypted-but-unparseable archive is treated as corruption: the
        // AEAD tag already verified, so this is a structural defect, not a
        // password issue — but to a caller it is still "this bundle is broken".
        ArchiveError::Malformed | ArchiveError::UnsafePath => {
            ConfigMigrationError::IncompatibleBundle {
                reason: "bundle archive is malformed".to_string(),
            }
        }
        ArchiveError::TooLarge => ConfigMigrationError::IncompatibleBundle {
            reason: "bundle archive exceeds the supported size".to_string(),
        },
    }
}

fn map_db_snapshot_err(err: DbSnapshotError) -> ConfigMigrationError {
    match err {
        DbSnapshotError::Connection | DbSnapshotError::Query => ConfigMigrationError::Internal {
            details: "database snapshot failed".to_string(),
        },
        DbSnapshotError::Io => ConfigMigrationError::Io {
            details: "database snapshot file io failed".to_string(),
        },
    }
}

/// Extract the salt + Argon2 parameters from a serialized keyslot.
///
/// The KEK is `Argon2id(passphrase, keyslot.salt, keyslot.kdf)`. Sealing a
/// bundle with the KEK and recording these same parameters in the bundle header
/// is what lets a reader reproduce the KEK from the space passphrase via
/// `bundle::open`. A keyslot that cannot be parsed, uses a non-Argon2id KDF, or
/// carries a salt of unexpected length is an internal inconsistency (an
/// initialized installation always has a well-formed V1 keyslot).
fn parse_keyslot_kdf(
    keyslot_bytes: &[u8],
) -> Result<([u8; 16], Argon2Params), ConfigMigrationError> {
    let keyslot: KeySlotFile =
        serde_json::from_slice(keyslot_bytes).map_err(|_| ConfigMigrationError::Internal {
            details: "keyslot could not be parsed for export".to_string(),
        })?;
    if keyslot.kdf.alg != "Argon2id" {
        return Err(ConfigMigrationError::Internal {
            details: "unsupported key-derivation algorithm in keyslot".to_string(),
        });
    }
    let salt: [u8; 16] =
        keyslot
            .salt
            .as_slice()
            .try_into()
            .map_err(|_| ConfigMigrationError::Internal {
                details: "keyslot salt has unexpected length".to_string(),
            })?;
    let kdf = Argon2Params {
        mem_kib: keyslot.kdf.params.mem_kib,
        iters: keyslot.kdf.params.iters,
        parallelism: keyslot.kdf.params.parallelism,
    };
    Ok((salt, kdf))
}

fn map_staging_err(err: StagingError) -> ConfigMigrationError {
    match err {
        StagingError::Io => ConfigMigrationError::Io {
            details: "writing the staged import failed".to_string(),
        },
        StagingError::Serialize => ConfigMigrationError::Internal {
            details: "encoding the staged import failed".to_string(),
        },
    }
}

#[async_trait]
impl ExportConfigBundlePort for ConfigMigrationAdapter {
    #[instrument(skip_all, fields(profile = %self.profile_id.inner()))]
    async fn export_bundle(&self, destination: &Path) -> Result<PathBuf, ConfigMigrationError> {
        info!("starting config bundle export");

        // 1. Consistent db snapshot (blocking; sqlite + file IO).
        let pool = self.db_pool.clone();
        let scratch = self.paths.snapshot_scratch_path();
        let db_bytes =
            tokio::task::spawn_blocking(move || db_snapshot::snapshot_to_bytes(&pool, &scratch))
                .await
                .map_err(|_| ConfigMigrationError::Internal {
                    details: "snapshot task failed to run".to_string(),
                })?
                .map_err(map_db_snapshot_err)?;

        // 2. Secrets (current-profile KEK only; identity is files). The KEK both
        //    encrypts this bundle and rides inside it for post-import
        //    auto-unlock. An unlocked session that lacks its KEK is a degenerate
        //    state we cannot seal from (the passphrase is never retained, so the
        //    KEK cannot be re-derived here) — fail rather than emit a bundle no
        //    passphrase can open.
        let (secret_entries, kek) = self.collect_secrets()?;
        let Some(kek_bytes) = kek else {
            error!("export aborted: current-profile KEK absent from secure storage while unlocked");
            return Err(ConfigMigrationError::Internal {
                details: "key material unavailable for export".to_string(),
            });
        };
        let kek_key: [u8; 32] = kek_bytes.as_slice().try_into().map_err(|_| {
            error!("export aborted: KEK material has unexpected length");
            ConfigMigrationError::Internal {
                details: "key material has unexpected length".to_string(),
            }
        })?;
        let secrets_file = SecretsFile::from_raw(secret_entries);

        // 3. iroh device-identity files (migrated as files, not a secret).
        let iroh_identity_files = self.collect_iroh_identity_files()?;

        // 4. Vault + settings files. The keyslot is carried verbatim and also
        //    parsed for the salt + KDF that derive the KEK from the passphrase,
        //    so the sealed bundle's header lets `open` reproduce that KEK.
        let keyslot = Self::read_required(&self.paths.vault_dir.join("keyslot.json"))?;
        let (bundle_salt, bundle_kdf) = parse_keyslot_kdf(&keyslot)?;
        let device_id = Self::read_required(&self.paths.vault_dir.join("device_id.txt"))?;
        // Setup-status marker (`vault/.setup_status`): on an initialized source
        // it should exist, but read it defensively (carry if present, skip if
        // absent) so a corner case where the marker was deleted but keyslot
        // remains does not abort the export. Its absence in the target after
        // apply would make the facade treat the installation as uninitialized.
        let setup_status = Self::read_optional(&self.paths.vault_dir.join(".setup_status"))?;
        let settings = Self::read_optional(&self.paths.settings_path)?;

        // 5. Device fingerprint (human-confirmable identity) + timestamp.
        let fingerprint = self
            .local_identity
            .get_current_fingerprint()
            .await
            .map_err(|_| ConfigMigrationError::Internal {
                details: "reading device fingerprint failed".to_string(),
            })?
            .map(|fp| fp.to_string())
            .unwrap_or_default();
        let created_at_unix_ms = self.clock.now_ms();

        // 6. Assemble the archive.
        let mut archive = BundleArchive::new();
        archive.insert(DB_MEMBER, db_bytes);
        archive.insert(KEYSLOT_MEMBER, keyslot);
        archive.insert(DEVICE_ID_MEMBER, device_id);
        if let Some(setup_status) = setup_status {
            archive.insert(SETUP_STATUS_MEMBER, setup_status);
        }
        if let Some(settings) = settings {
            archive.insert(SETTINGS_MEMBER, settings);
        }
        archive.insert(
            SECRETS_MEMBER,
            secrets_file.to_json_bytes().map_err(map_staging_err)?,
        );
        for (member, bytes) in iroh_identity_files {
            archive.insert(member, bytes);
        }
        for (name, path) in self.paths.ui_state_paths() {
            if let Some(bytes) = Self::read_optional(&path)? {
                archive.insert(format!("{UI_STATE_PREFIX}{name}"), bytes);
            }
        }

        let manifest = BundleManifest {
            schema_ver: MANIFEST_SCHEMA_VER,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            source_mode: Self::source_mode(),
            created_at_unix_ms,
            profile_id: self.profile_id.inner().clone(),
            device_fingerprint: fingerprint,
            included: archive.member_paths(),
        };
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|_| ConfigMigrationError::Internal {
                details: "encoding the bundle manifest failed".to_string(),
            })?;
        // `included` is computed before the manifest member is added; reinsert
        // with the manifest now present so the recorded list is accurate.
        archive.insert(MANIFEST_MEMBER, manifest_bytes);
        let manifest = BundleManifest {
            included: archive.member_paths(),
            ..manifest
        };
        archive.insert(
            MANIFEST_MEMBER,
            serde_json::to_vec_pretty(&manifest).map_err(|_| ConfigMigrationError::Internal {
                details: "encoding the bundle manifest failed".to_string(),
            })?,
        );

        // 6. Tar → seal → write. The bundle is sealed with the KEK directly
        //    (no Argon2 here — the salt + KDF written to the header let `open`
        //    re-derive the KEK from the passphrase). AEAD over the snapshot is
        //    still CPU-bound, so run it blocking.
        let tar_bytes = archive.to_tar_bytes().map_err(map_archive_err)?;
        let sealed = tokio::task::spawn_blocking(move || {
            bundle::seal_with_key(&kek_key, &bundle_salt, bundle_kdf, &tar_bytes)
        })
        .await
        .map_err(|_| ConfigMigrationError::Internal {
            details: "bundle seal task failed to run".to_string(),
        })?
        .map_err(map_bundle_err)?;

        let destination = destination.to_path_buf();
        let final_path = tokio::task::spawn_blocking(move || {
            if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|_| ConfigMigrationError::Io {
                    details: "failed creating the destination directory".to_string(),
                })?;
            }
            let tmp = destination.with_extension("ucbundle.tmp");
            std::fs::write(&tmp, &sealed).map_err(|_| ConfigMigrationError::Io {
                details: "failed writing the bundle file".to_string(),
            })?;
            std::fs::rename(&tmp, &destination).map_err(|_| ConfigMigrationError::Io {
                details: "failed finalizing the bundle file".to_string(),
            })?;
            Ok::<_, ConfigMigrationError>(destination)
        })
        .await
        .map_err(|_| ConfigMigrationError::Internal {
            details: "bundle write task failed to run".to_string(),
        })??;

        info!("config bundle export complete");
        Ok(final_path)
    }
}

#[async_trait]
impl PreviewConfigImportPort for ConfigMigrationAdapter {
    #[instrument(skip_all)]
    async fn preview_import(
        &self,
        password: &Passphrase,
        source: &Path,
    ) -> Result<ConfigImportPreview, ConfigMigrationError> {
        let (_, manifest) = self.open_archive(password, source).await?;

        let source_mode = match manifest.source_mode {
            ManifestSourceMode::Portable => ConfigSourceMode::Portable,
            ManifestSourceMode::Installed => ConfigSourceMode::Installed,
        };

        Ok(ConfigImportPreview {
            app_version: manifest.app_version,
            source_mode,
            created_at_unix_ms: manifest.created_at_unix_ms,
            profile_id: ProfileId::from(manifest.profile_id),
            device_fingerprint: manifest.device_fingerprint,
        })
    }
}

#[async_trait]
impl StageConfigImportPort for ConfigMigrationAdapter {
    #[instrument(skip_all)]
    async fn stage_import(
        &self,
        password: &Passphrase,
        source: &Path,
    ) -> Result<StagedConfigImport, ConfigMigrationError> {
        info!("staging config import");
        let (archive, _manifest) = self.open_archive(password, source).await?;

        // Decide whether a KEK is present by inspecting the staged secrets.
        let has_kek = archive
            .get(SECRETS_MEMBER)
            .and_then(|bytes| serde_json::from_slice::<SecretsFile>(bytes).ok())
            .map(|secrets| {
                secrets
                    .secrets
                    .keys()
                    .any(|k| k.starts_with("kek:v1:profile:"))
            })
            .unwrap_or(false);

        let data_root = self.paths.app_data_root.clone();
        let staged_at_unix_ms = self.clock.now_ms();
        let marker = PendingImportMarker {
            schema_ver: PENDING_IMPORT_SCHEMA_VER,
            staging_dir: STAGING_DIR_NAME.to_string(),
            has_kek,
            staged_at_unix_ms,
        };

        // Filesystem writes are blocking.
        tokio::task::spawn_blocking(move || {
            let layout = StagingLayout::new(data_root);
            layout.write(&archive, &marker)
        })
        .await
        .map_err(|_| ConfigMigrationError::Internal {
            details: "staging task failed to run".to_string(),
        })?
        .map_err(map_staging_err)?;

        if !has_kek {
            warn!("staged bundle carried no KEK; unlock will be required after apply");
        }
        info!(has_kek, "config import staged");

        Ok(StagedConfigImport {
            unlock_required_after_apply: !has_kek,
        })
    }
}
