//! Import staging layout and the `pending-import.json` marker.
//!
//! Staging writes an unpacked, validated bundle under the data root so a later
//! restart can adopt it. This module owns the on-disk contract consumed by the
//! boot-time apply step: the directory layout, the marker schema, and the
//! `secrets.json` format. Those are persistence invariants — bump
//! [`PENDING_IMPORT_SCHEMA_VER`] before changing them.
//!
//! Layout under the data root:
//!
//! ```text
//! import-staging/
//!   manifest.json          # copied verbatim from the bundle archive
//!   db/uniclipboard.db     # consistent snapshot to install as the live db
//!   vault/keyslot.json     # keyslot to install into the vault dir
//!   vault/device_id.txt
//!   vault/.setup_status    # "is initialized" marker; copy back into vault dir
//!   iroh-identity/*        # 0600 device-identity files; copy into identity dir
//!   settings.json
//!   secrets.json           # { "secrets": { "<key>": "<base64>" , ... } }; KEK only
//!   ui-state/*.json        # optional
//! pending-import.json      # marker at the data root (sibling of import-staging/)
//! ```
//!
//! The marker is a *sibling* of the staging directory (not inside it) so its
//! presence/absence is the single boot-time signal, independent of the staging
//! contents.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};

use super::archive::BundleArchive;

/// Directory name (under the data root) holding the unpacked bundle.
pub const STAGING_DIR_NAME: &str = "import-staging";

/// Marker file name (under the data root) signalling a pending import.
pub const PENDING_IMPORT_MARKER: &str = "pending-import.json";

/// Schema version of the marker + staging layout contract.
pub const PENDING_IMPORT_SCHEMA_VER: u32 = 1;

/// Boot-time marker written next to the staging directory.
///
/// Field names are the on-disk contract read by the boot-time apply step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingImportMarker {
    /// Marker/staging-layout schema version.
    pub schema_ver: u32,
    /// Staging directory name, relative to the data root (always
    /// [`STAGING_DIR_NAME`]). Recorded explicitly so the apply step never has
    /// to assume it.
    pub staging_dir: String,
    /// Whether the staged bundle carried a KEK. When `false`, applying still
    /// installs the device identity but the operator must unlock with the
    /// passphrase afterwards.
    pub has_kek: bool,
    /// When the import was staged, milliseconds since the Unix epoch (from the
    /// staging operation, not the bundle's own creation time).
    pub staged_at_unix_ms: i64,
}

/// The `secrets.json` member: secure-storage key → base64-encoded raw value.
///
/// Wrapped in a struct (rather than a bare map) so the format can grow a
/// version/envelope later without breaking the member shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretsFile {
    /// Map of secure-storage key string → standard base64 of the raw secret
    /// bytes.
    pub secrets: BTreeMap<String, String>,
}

impl SecretsFile {
    /// Build a secrets file from raw key→bytes pairs (base64-encodes values).
    pub fn from_raw(entries: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        let engine = base64::engine::general_purpose::STANDARD;
        let secrets = entries
            .into_iter()
            .map(|(k, v)| (k, engine.encode(v)))
            .collect();
        Self { secrets }
    }

    /// Serialize to pretty JSON bytes.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, StagingError> {
        serde_json::to_vec_pretty(self).map_err(|_| StagingError::Serialize)
    }
}

/// Staging-side failures (path/IO/serialization). The adapter maps these onto
/// the domain error.
#[derive(Debug, thiserror::Error)]
pub enum StagingError {
    /// Filesystem write/cleanup failed.
    #[error("staging io failed")]
    Io,
    /// Marker or member serialization failed.
    #[error("staging serialize failed")]
    Serialize,
}

/// Filesystem layout helper for the staging area, rooted at the data root.
pub struct StagingLayout {
    data_root: PathBuf,
}

impl StagingLayout {
    pub fn new(data_root: impl Into<PathBuf>) -> Self {
        Self {
            data_root: data_root.into(),
        }
    }

    /// The staging directory: `<data_root>/import-staging/`.
    pub fn staging_dir(&self) -> PathBuf {
        self.data_root.join(STAGING_DIR_NAME)
    }

    /// The pending-import marker path: `<data_root>/pending-import.json`.
    pub fn marker_path(&self) -> PathBuf {
        self.data_root.join(PENDING_IMPORT_MARKER)
    }

    /// Write an unpacked archive into a *fresh* staging directory, then write
    /// the marker last so a crash mid-extraction never leaves a marker pointing
    /// at a half-written staging area.
    ///
    /// Any pre-existing staging directory or marker is cleared first so a
    /// retried import starts clean.
    pub fn write(
        &self,
        archive: &BundleArchive,
        marker: &PendingImportMarker,
    ) -> Result<(), StagingError> {
        let staging = self.staging_dir();
        let marker_path = self.marker_path();

        // Clear any prior attempt: marker first (so a crash here leaves no
        // marker), then the directory.
        if marker_path.exists() {
            std::fs::remove_file(&marker_path).map_err(|_| StagingError::Io)?;
        }
        if staging.exists() {
            std::fs::remove_dir_all(&staging).map_err(|_| StagingError::Io)?;
        }
        std::fs::create_dir_all(&staging).map_err(|_| StagingError::Io)?;

        for (member, bytes) in archive.iter() {
            let dest = staging.join(member);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|_| StagingError::Io)?;
            }
            std::fs::write(&dest, bytes).map_err(|_| StagingError::Io)?;
        }

        // Marker written last and atomically (tmp + rename) so its presence
        // implies a fully-written staging directory.
        let marker_json = serde_json::to_vec_pretty(marker).map_err(|_| StagingError::Serialize)?;
        let tmp = marker_path.with_extension("json.tmp");
        std::fs::write(&tmp, &marker_json).map_err(|_| StagingError::Io)?;
        std::fs::rename(&tmp, &marker_path).map_err(|_| StagingError::Io)?;

        Ok(())
    }
}

/// Decode a base64 secret value from a [`SecretsFile`] entry.
///
/// Exposed so the boot-time apply step shares the exact decoding contract used
/// by the writer.
pub fn decode_secret_value(b64: &str) -> Result<Vec<u8>, StagingError> {
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|_| StagingError::Serialize)
}

/// Member path of the keyslot inside the bundle / staging area.
pub const KEYSLOT_MEMBER: &str = "vault/keyslot.json";

/// Member path of the device id inside the bundle / staging area.
pub const DEVICE_ID_MEMBER: &str = "vault/device_id.txt";

/// Member path of the setup-status marker inside the bundle / staging area.
///
/// `FileSetupStatusRepository` persists `SetupStatus { has_completed, space_id }`
/// to `vault_dir/.setup_status`. The facade reads `has_completed` as its
/// "is this installation initialized" source of truth, so the marker must
/// travel with the bundle — without it, an imported installation that holds all
/// its data would still be treated as uninitialized and re-prompt setup.
pub const SETUP_STATUS_MEMBER: &str = "vault/.setup_status";

/// Member path of settings inside the bundle / staging area.
pub const SETTINGS_MEMBER: &str = "settings.json";

/// Member path of the db snapshot inside the bundle / staging area.
pub const DB_MEMBER: &str = "db/uniclipboard.db";

/// Prefix for optional UI-state members.
pub const UI_STATE_PREFIX: &str = "ui-state/";

/// Prefix for iroh device-identity files.
///
/// The iroh identity is *not* a user secret kept in the credential store; it is
/// persisted as `0600` files in a dedicated directory (a `FileSecureStorage`
/// backend) so startup never prompts a keychain dialog. It therefore migrates
/// as files (like `vault/`), not as a `secrets.json` entry: every file in the
/// source identity directory is carried under this prefix and the boot step
/// copies them back into the target identity directory.
pub const IROH_IDENTITY_PREFIX: &str = "iroh-identity/";

/// Resolve a staging-relative member to an absolute path under `data_root`'s
/// staging directory. Used by tests and by the boot-time apply step.
pub fn staged_member_path(data_root: &Path, member: &str) -> PathBuf {
    data_root.join(STAGING_DIR_NAME).join(member)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_file_round_trips_base64() {
        // Arbitrary key strings — this exercises the generic container; the only
        // real secret carried today is the current-profile KEK.
        let file = SecretsFile::from_raw([
            ("kek:v1:profile:default".to_string(), vec![9u8, 8, 7]),
            ("kek:v1:profile:other".to_string(), vec![1u8, 2, 3]),
        ]);
        let json = file.to_json_bytes().unwrap();
        let back: SecretsFile = serde_json::from_slice(&json).unwrap();

        let kek = back.secrets.get("kek:v1:profile:default").unwrap();
        assert_eq!(decode_secret_value(kek).unwrap(), vec![9u8, 8, 7]);
        let other = back.secrets.get("kek:v1:profile:other").unwrap();
        assert_eq!(decode_secret_value(other).unwrap(), vec![1u8, 2, 3]);
    }

    #[test]
    fn write_lays_out_staging_and_marker() {
        let dir = tempfile::tempdir().unwrap();
        let layout = StagingLayout::new(dir.path());

        let mut archive = BundleArchive::new();
        archive.insert("manifest.json", b"{}".to_vec());
        archive.insert(DB_MEMBER, vec![0u8; 8]);
        archive.insert(KEYSLOT_MEMBER, b"{\"version\":\"V1\"}".to_vec());

        let marker = PendingImportMarker {
            schema_ver: PENDING_IMPORT_SCHEMA_VER,
            staging_dir: STAGING_DIR_NAME.to_string(),
            has_kek: true,
            staged_at_unix_ms: 1_700_000_000_000,
        };

        layout.write(&archive, &marker).unwrap();

        assert!(layout.marker_path().exists());
        assert!(staged_member_path(dir.path(), "manifest.json").exists());
        assert!(staged_member_path(dir.path(), DB_MEMBER).exists());
        assert!(staged_member_path(dir.path(), KEYSLOT_MEMBER).exists());

        let read_marker: PendingImportMarker =
            serde_json::from_slice(&std::fs::read(layout.marker_path()).unwrap()).unwrap();
        assert_eq!(read_marker, marker);
    }

    #[test]
    fn write_clears_prior_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let layout = StagingLayout::new(dir.path());

        // Seed a stale staging dir with a file that should not survive.
        std::fs::create_dir_all(layout.staging_dir()).unwrap();
        std::fs::write(layout.staging_dir().join("stale.txt"), b"old").unwrap();

        let mut archive = BundleArchive::new();
        archive.insert("manifest.json", b"{}".to_vec());
        let marker = PendingImportMarker {
            schema_ver: PENDING_IMPORT_SCHEMA_VER,
            staging_dir: STAGING_DIR_NAME.to_string(),
            has_kek: false,
            staged_at_unix_ms: 1,
        };
        layout.write(&archive, &marker).unwrap();

        assert!(!staged_member_path(dir.path(), "stale.txt").exists());
        assert!(staged_member_path(dir.path(), "manifest.json").exists());
    }
}
