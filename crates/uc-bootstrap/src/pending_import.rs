//! Boot-time application of a staged configuration import.
//!
//! A successful `/config/import` leaves an unpacked, validated bundle under the
//! data root (`import-staging/`) plus a `pending-import.json` marker (the Unit 2
//! staging contract in `uc_infra::config_migration::staging`). The very next
//! daemon boot adopts it here, before the db pool is opened and before secure
//! storage is consumed by key material / identity wiring.
//!
//! This is the bridge across "gap 3" from the design doc: the staged
//! `secrets.json` (portable iroh identity + optional KEK) is written into the
//! *current* secure-storage backend — whichever backend the rest of wiring
//! selected (installer ⇒ Credential Manager / Keychain, portable ⇒ file). That
//! is what lets a portable→installer migration keep the same network identity
//! without re-pairing.
//!
//! ## Crash-safety / idempotency (design doc §8)
//!
//! Order is fixed so a crash never leaves "db swapped but identity not written"
//! (which would silently mint a fresh NodeId and force re-pairing — the exact
//! outcome the feature exists to avoid):
//!
//! 1. Write every staged secret into the current backend. `set` is an
//!    idempotent overwrite. If any secret fails, abort: copy no files, keep the
//!    staging directory + marker, log an error, and let boot continue
//!    uninitialized. The next boot retries from the intact marker.
//! 2. Only after all secrets land, copy the file members (db / keyslot /
//!    device-id / setup-status / settings / ui-state) into their live
//!    locations. File copy is an idempotent overwrite.
//! 3. Only after all copies succeed, delete the staging directory + marker.
//!
//! A schema-version mismatch on the marker is *not* fatal: it is logged and the
//! staging area is preserved untouched so a newer build (or the user) can deal
//! with it, never blocking boot.

use std::path::Path;
use std::sync::Arc;

use tracing::{error, info};

use uc_core::ports::SecureStoragePort;
use uc_infra::config_migration::secret_keys::SECRETS_MEMBER;
use uc_infra::config_migration::staging::{
    decode_secret_value, PendingImportMarker, SecretsFile, StagingLayout, DB_MEMBER,
    DEVICE_ID_MEMBER, KEYSLOT_MEMBER, PENDING_IMPORT_SCHEMA_VER, SETTINGS_MEMBER,
    SETUP_STATUS_MEMBER, UI_STATE_PREFIX,
};

/// Secure-storage key prefixes whose presence may be logged (no values ever
/// leave the process). Anything else is reported as `unknown`.
const KNOWN_SECRET_PREFIXES: &[&str] = &["iroh-identity:", "kek:v1:"];

/// Detect and apply a staged configuration import, if one is pending.
///
/// `app_data_root` is the staging/marker root (and the parent of `ui-state/`);
/// `db_path` / `vault_dir` / `settings_path` are the live destinations for the
/// copied members. `secure_storage` must be the *same* backend the rest of
/// wiring uses so secrets land where the running daemon will later read them.
///
/// Returns `Ok(())` in the common (no-marker) case with zero filesystem work,
/// and also when a recoverable problem (schema mismatch, secret-write failure)
/// is logged but boot should continue uninitialized with the staging preserved.
/// Only an unexpected failure while reading the marker / staged members or while
/// cleaning up surfaces as `Err`.
pub fn apply_pending_import(
    app_data_root: &Path,
    db_path: &Path,
    vault_dir: &Path,
    settings_path: &Path,
    secure_storage: &Arc<dyn SecureStoragePort>,
) -> Result<(), PendingImportError> {
    let layout = StagingLayout::new(app_data_root);
    let marker_path = layout.marker_path();

    // Common case: no marker. Zero overhead, no logging noise.
    if !marker_path.exists() {
        return Ok(());
    }

    info!("pending config import detected; applying staged bundle on boot");

    let marker_bytes = std::fs::read(&marker_path).map_err(|_| PendingImportError::ReadMarker)?;
    let marker: PendingImportMarker =
        serde_json::from_slice(&marker_bytes).map_err(|_| PendingImportError::ParseMarker)?;

    // Schema mismatch: do not apply, do not delete — preserve for a build that
    // understands this version. Never block boot.
    if marker.schema_ver != PENDING_IMPORT_SCHEMA_VER {
        error!(
            found_schema_ver = marker.schema_ver,
            expected_schema_ver = PENDING_IMPORT_SCHEMA_VER,
            "staged import schema version mismatch; skipping apply and preserving staging"
        );
        return Ok(());
    }

    let staging_dir = layout.staging_dir();

    // --- Step 1: secrets first. ---------------------------------------------
    // Parse the staged secrets, then write each into the current backend. A
    // failure here aborts the whole apply (no files copied, staging kept).
    let secrets_path = staging_dir.join(SECRETS_MEMBER);
    let secrets_bytes =
        std::fs::read(&secrets_path).map_err(|_| PendingImportError::ReadSecrets)?;
    let secrets: SecretsFile =
        serde_json::from_slice(&secrets_bytes).map_err(|_| PendingImportError::ParseSecrets)?;

    info!(
        secret_count = secrets.secrets.len(),
        has_kek = marker.has_kek,
        "writing staged secrets into current secure-storage backend"
    );

    for (key, b64) in &secrets.secrets {
        let bytes = match decode_secret_value(b64) {
            Ok(bytes) => bytes,
            Err(_) => {
                // Recoverable: leave staging intact for a retry; do not copy
                // any files (avoids the "db swapped, identity missing" half
                // state). Boot continues uninitialized.
                error!(
                    key_class = classify_secret_key(key),
                    "staged secret value failed to decode; aborting import apply, staging preserved"
                );
                return Ok(());
            }
        };

        if let Err(err) = secure_storage.set(key, &bytes) {
            error!(
                key_class = classify_secret_key(key),
                error = %err,
                "writing staged secret into secure storage failed; aborting import apply, staging preserved"
            );
            return Ok(());
        }
    }

    info!("staged secrets written; copying staged files into live locations");

    // --- Step 2: copy file members into their live locations. ----------------
    // Required members.
    copy_member(&staging_dir, DB_MEMBER, db_path)?;
    copy_member(
        &staging_dir,
        KEYSLOT_MEMBER,
        &vault_dir.join("keyslot.json"),
    )?;
    copy_member(
        &staging_dir,
        DEVICE_ID_MEMBER,
        &vault_dir.join("device_id.txt"),
    )?;
    copy_member(
        &staging_dir,
        SETUP_STATUS_MEMBER,
        &vault_dir.join(".setup_status"),
    )?;

    // Optional members: skip silently when absent.
    copy_member_if_present(&staging_dir, SETTINGS_MEMBER, settings_path)?;
    copy_ui_state(&staging_dir, app_data_root)?;

    // --- Step 3: clean up only after everything landed. ----------------------
    std::fs::remove_dir_all(&staging_dir).map_err(|_| PendingImportError::Cleanup)?;
    std::fs::remove_file(&marker_path).map_err(|_| PendingImportError::Cleanup)?;

    info!("staged config import applied; staging cleaned up");
    Ok(())
}

/// Copy a required staged member to `dest`, creating parent dirs as needed.
fn copy_member(staging_dir: &Path, member: &str, dest: &Path) -> Result<(), PendingImportError> {
    let src = staging_dir.join(member);
    ensure_parent(dest)?;
    std::fs::copy(&src, dest).map_err(|_| PendingImportError::CopyMember)?;
    Ok(())
}

/// Copy an optional staged member; absence is not an error.
fn copy_member_if_present(
    staging_dir: &Path,
    member: &str,
    dest: &Path,
) -> Result<(), PendingImportError> {
    let src = staging_dir.join(member);
    if !src.exists() {
        return Ok(());
    }
    ensure_parent(dest)?;
    std::fs::copy(&src, dest).map_err(|_| PendingImportError::CopyMember)?;
    Ok(())
}

/// Copy every `ui-state/*` member to `<app_data_root>/ui-state/*`. The whole
/// prefix is optional.
fn copy_ui_state(staging_dir: &Path, app_data_root: &Path) -> Result<(), PendingImportError> {
    let prefix = UI_STATE_PREFIX.trim_end_matches('/');
    let src_dir = staging_dir.join(prefix);
    if !src_dir.is_dir() {
        return Ok(());
    }

    let dest_dir = app_data_root.join(prefix);
    std::fs::create_dir_all(&dest_dir).map_err(|_| PendingImportError::CopyMember)?;

    let entries = std::fs::read_dir(&src_dir).map_err(|_| PendingImportError::CopyMember)?;
    for entry in entries {
        let entry = entry.map_err(|_| PendingImportError::CopyMember)?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let dest = dest_dir.join(entry.file_name());
        std::fs::copy(entry.path(), &dest).map_err(|_| PendingImportError::CopyMember)?;
    }
    Ok(())
}

/// Ensure the parent directory of `dest` exists.
fn ensure_parent(dest: &Path) -> Result<(), PendingImportError> {
    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|_| PendingImportError::CopyMember)?;
    }
    Ok(())
}

/// Classify a secret key for logging without exposing its value. Returns the
/// matching known prefix or `"unknown"`; never the full key (which already
/// embeds a profile id) and never the value.
fn classify_secret_key(key: &str) -> &'static str {
    for prefix in KNOWN_SECRET_PREFIXES {
        if key.starts_with(prefix) {
            return prefix;
        }
    }
    "unknown"
}

/// Failures while applying a staged import.
///
/// These cover genuinely unexpected IO/parse failures (reading the marker,
/// copying members, cleanup). Recoverable conditions — no marker, schema
/// mismatch, secret decode/write failure — are handled in-band (logged + staging
/// preserved) and return `Ok(())`, so they never abort boot.
#[derive(Debug, thiserror::Error)]
pub enum PendingImportError {
    #[error("failed to read pending-import marker")]
    ReadMarker,
    #[error("failed to parse pending-import marker")]
    ParseMarker,
    #[error("failed to read staged secrets")]
    ReadSecrets,
    #[error("failed to parse staged secrets")]
    ParseSecrets,
    #[error("failed to copy a staged member into its live location")]
    CopyMember,
    #[error("failed to clean up staging after applying import")]
    Cleanup,
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::sync::Mutex;

    use uc_core::ports::SecureStorageError;
    use uc_infra::config_migration::staging::PENDING_IMPORT_MARKER;

    /// In-memory secure storage with an optional fail-on-set switch so the
    /// "secrets write fails" path can be exercised.
    #[derive(Default)]
    struct TestSecureStorage {
        map: Mutex<HashMap<String, Vec<u8>>>,
        fail_set: Mutex<bool>,
    }

    impl TestSecureStorage {
        fn fail_next_sets(&self) {
            *self.fail_set.lock().unwrap() = true;
        }
    }

    impl SecureStoragePort for TestSecureStorage {
        fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(self.map.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
            if *self.fail_set.lock().unwrap() {
                return Err(SecureStorageError::Other("forced failure".to_string()));
            }
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

    /// Lay out a complete staging area + marker under `data_root`.
    fn seed_staging(data_root: &Path, has_kek: bool) {
        let layout = StagingLayout::new(data_root);
        let staging = layout.staging_dir();

        // Required file members.
        std::fs::create_dir_all(staging.join("db")).unwrap();
        std::fs::write(staging.join(DB_MEMBER), b"DBSNAPSHOT").unwrap();

        std::fs::create_dir_all(staging.join("vault")).unwrap();
        std::fs::write(staging.join(KEYSLOT_MEMBER), b"{\"version\":\"V1\"}").unwrap();
        std::fs::write(staging.join(DEVICE_ID_MEMBER), b"device-uuid").unwrap();
        std::fs::write(
            staging.join(SETUP_STATUS_MEMBER),
            b"{\"has_completed\":true,\"space_id\":null}",
        )
        .unwrap();

        // Optional members.
        std::fs::write(staging.join(SETTINGS_MEMBER), b"{\"schema_version\":1}").unwrap();
        std::fs::create_dir_all(staging.join("ui-state")).unwrap();
        std::fs::write(staging.join("ui-state/last_notified_update.json"), b"{}").unwrap();

        // secrets.json — built via the staging contract's own encoder so the
        // base64 layout matches exactly what the importer wrote.
        let mut raw: Vec<(String, Vec<u8>)> = vec![("iroh-identity:v1".to_string(), vec![7u8; 32])];
        if has_kek {
            raw.push(("kek:v1:profile:default".to_string(), vec![9u8; 32]));
        }
        let secrets_file = SecretsFile::from_raw(raw);
        std::fs::write(
            staging.join(SECRETS_MEMBER),
            secrets_file.to_json_bytes().unwrap(),
        )
        .unwrap();

        // Marker last.
        let marker = PendingImportMarker {
            schema_ver: PENDING_IMPORT_SCHEMA_VER,
            staging_dir: "import-staging".to_string(),
            has_kek,
            staged_at_unix_ms: 1_700_000_000_000,
        };
        std::fs::write(
            layout.marker_path(),
            serde_json::to_vec_pretty(&marker).unwrap(),
        )
        .unwrap();
    }

    struct Dests {
        db_path: std::path::PathBuf,
        vault_dir: std::path::PathBuf,
        settings_path: std::path::PathBuf,
    }

    fn live_dests(root: &Path) -> Dests {
        Dests {
            db_path: root.join("live/uniclipboard.db"),
            vault_dir: root.join("vault"),
            settings_path: root.join("settings.json"),
        }
    }

    #[test]
    fn no_marker_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let data_root = dir.path();
        let storage: Arc<dyn SecureStoragePort> = Arc::new(TestSecureStorage::default());
        let d = live_dests(data_root);

        apply_pending_import(
            data_root,
            &d.db_path,
            &d.vault_dir,
            &d.settings_path,
            &storage,
        )
        .expect("no-marker path must be ok");

        assert!(!d.db_path.exists());
    }

    #[test]
    fn applies_files_and_secrets_then_cleans_up_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let data_root = dir.path();
        seed_staging(data_root, true);

        let storage_concrete = Arc::new(TestSecureStorage::default());
        let storage: Arc<dyn SecureStoragePort> = storage_concrete.clone();
        let d = live_dests(data_root);

        apply_pending_import(
            data_root,
            &d.db_path,
            &d.vault_dir,
            &d.settings_path,
            &storage,
        )
        .expect("apply should succeed");

        // (1) Files landed in their live locations.
        assert_eq!(std::fs::read(&d.db_path).unwrap(), b"DBSNAPSHOT");
        assert_eq!(
            std::fs::read(d.vault_dir.join("keyslot.json")).unwrap(),
            b"{\"version\":\"V1\"}"
        );
        assert_eq!(
            std::fs::read(d.vault_dir.join("device_id.txt")).unwrap(),
            b"device-uuid"
        );
        assert_eq!(
            std::fs::read(d.vault_dir.join(".setup_status")).unwrap(),
            b"{\"has_completed\":true,\"space_id\":null}"
        );
        assert_eq!(
            std::fs::read(&d.settings_path).unwrap(),
            b"{\"schema_version\":1}"
        );
        assert_eq!(
            std::fs::read(data_root.join("ui-state/last_notified_update.json")).unwrap(),
            b"{}"
        );

        // (2) Secrets written into the backend.
        assert_eq!(
            storage_concrete.get("iroh-identity:v1").unwrap().unwrap(),
            vec![7u8; 32]
        );
        assert_eq!(
            storage_concrete
                .get("kek:v1:profile:default")
                .unwrap()
                .unwrap(),
            vec![9u8; 32]
        );

        // (3) Staging + marker cleaned up.
        let layout = StagingLayout::new(data_root);
        assert!(!layout.staging_dir().exists());
        assert!(!layout.marker_path().exists());
        assert!(!data_root.join(PENDING_IMPORT_MARKER).exists());

        // (4) Second call is a no-op (marker gone).
        apply_pending_import(
            data_root,
            &d.db_path,
            &d.vault_dir,
            &d.settings_path,
            &storage,
        )
        .expect("second apply must be a no-op");
    }

    #[test]
    fn secret_write_failure_aborts_apply_and_preserves_staging() {
        let dir = tempfile::tempdir().unwrap();
        let data_root = dir.path();
        seed_staging(data_root, true);

        let storage_concrete = Arc::new(TestSecureStorage::default());
        storage_concrete.fail_next_sets();
        let storage: Arc<dyn SecureStoragePort> = storage_concrete.clone();
        let d = live_dests(data_root);

        // Recoverable: returns Ok but copies nothing and keeps staging.
        apply_pending_import(
            data_root,
            &d.db_path,
            &d.vault_dir,
            &d.settings_path,
            &storage,
        )
        .expect("secret-write failure is handled in-band (Ok)");

        // No file was copied (no "db swapped but identity missing" half state).
        assert!(!d.db_path.exists());
        assert!(!d.vault_dir.join("keyslot.json").exists());
        assert!(!d.settings_path.exists());

        // Staging + marker preserved for the next boot's retry.
        let layout = StagingLayout::new(data_root);
        assert!(layout.staging_dir().exists());
        assert!(layout.marker_path().exists());
    }

    #[test]
    fn schema_mismatch_skips_apply_and_preserves_staging() {
        let dir = tempfile::tempdir().unwrap();
        let data_root = dir.path();
        seed_staging(data_root, true);

        // Rewrite the marker with an unsupported schema version.
        let layout = StagingLayout::new(data_root);
        let bumped = PendingImportMarker {
            schema_ver: PENDING_IMPORT_SCHEMA_VER + 1,
            staging_dir: "import-staging".to_string(),
            has_kek: true,
            staged_at_unix_ms: 1,
        };
        std::fs::write(
            layout.marker_path(),
            serde_json::to_vec_pretty(&bumped).unwrap(),
        )
        .unwrap();

        let storage: Arc<dyn SecureStoragePort> = Arc::new(TestSecureStorage::default());
        let d = live_dests(data_root);

        apply_pending_import(
            data_root,
            &d.db_path,
            &d.vault_dir,
            &d.settings_path,
            &storage,
        )
        .expect("schema mismatch must not block boot");

        // Nothing applied, staging preserved.
        assert!(!d.db_path.exists());
        assert!(layout.staging_dir().exists());
        assert!(layout.marker_path().exists());
    }
}
