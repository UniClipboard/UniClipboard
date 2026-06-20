//! `manifest.json` — the descriptive, non-secret metadata carried inside a
//! decrypted bundle archive.
//!
//! The manifest is the *inner* version record (the `.ucbundle` plaintext header
//! carries the outer `format_ver`; see [`super::bundle`]). It pins a
//! `schema_ver` so a future archive-layout change can be detected and rejected
//! with a stable reason instead of mis-parsing.
//!
//! Persisted format invariants (do not reorder/rename fields without bumping
//! `schema_ver`): serde field names are the on-disk contract.

use serde::{Deserialize, Serialize};

/// Current manifest schema version.
///
/// Bump when the archive layout (member set / manifest shape) changes in a way
/// older readers cannot understand. Readers reject anything greater than the
/// version they were built with.
pub const MANIFEST_SCHEMA_VER: u32 = 1;

/// Storage layout the source installation ran under, recorded so the operator
/// (and the staging step) can reason about cross-layout migration.
///
/// Mirrors the domain `ConfigSourceMode` but is owned here because the manifest
/// is a persistence format detail; the adapter maps between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestSourceMode {
    /// Self-contained layout that keeps all data next to the executable.
    Portable,
    /// Layout that keeps data in the platform's per-user application data
    /// location.
    Installed,
}

/// Descriptive metadata inside a bundle archive.
///
/// Carries only non-secret fields. It never embeds key material; secrets live
/// in the archive's `secrets.json` member, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    /// Inner archive schema version. See [`MANIFEST_SCHEMA_VER`].
    pub schema_ver: u32,
    /// Application version string of the producing installation.
    pub app_version: String,
    /// Storage layout the bundle was produced under.
    pub source_mode: ManifestSourceMode,
    /// Bundle creation time, milliseconds since the Unix epoch.
    pub created_at_unix_ms: i64,
    /// Profile the bundle's configuration belongs to.
    pub profile_id: String,
    /// Stable, human-comparable identity fingerprint of the producing device.
    pub device_fingerprint: String,
    /// Archive member paths that were actually included, for diagnostics and
    /// forward-compatible inspection.
    pub included: Vec<String>,
}

/// File name of the manifest member inside the archive.
pub const MANIFEST_MEMBER: &str = "manifest.json";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips_through_json() {
        let manifest = BundleManifest {
            schema_ver: MANIFEST_SCHEMA_VER,
            app_version: "0.16.0".to_string(),
            source_mode: ManifestSourceMode::Portable,
            created_at_unix_ms: 1_700_000_000_000,
            profile_id: "default".to_string(),
            device_fingerprint: "ABCD-EFGH-IJKL-MNOP".to_string(),
            included: vec!["db/uniclipboard.db".to_string()],
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let back: BundleManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, back);
    }

    #[test]
    fn source_mode_serializes_to_stable_snake_case() {
        let json = serde_json::to_string(&ManifestSourceMode::Installed).unwrap();
        assert_eq!(json, "\"installed\"");
    }
}
