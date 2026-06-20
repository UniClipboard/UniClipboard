//! Domain model for whole-installation configuration migration.
//!
//! These types describe the *intent* and *outcome* of moving a complete
//! installation's configuration from one machine (or storage layout) to
//! another. They deliberately stay free of any packaging, encryption, key
//! derivation, key naming, file-format, or persistence details — those are
//! infrastructure concerns and must never surface here.

use crate::ids::ProfileId;

/// Storage layout an installation runs under.
///
/// This distinguishes a self-contained, no-trace installation that keeps all
/// its data alongside the executable from one that stores its data in the
/// platform's per-user application data location. Migration may cross between
/// the two layouts, so a bundle records which layout produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSourceMode {
    /// Self-contained layout that keeps all data next to the executable and
    /// leaves no trace in shared per-user locations.
    Portable,
    /// Layout that keeps data in the platform's per-user application data
    /// location.
    Installed,
}

/// Human-confirmable preview of a configuration bundle.
///
/// Carries only the non-secret descriptive metadata an operator needs to
/// decide whether to proceed with an import. It never exposes any of the
/// secret material the bundle protects, and it omits any packaging or
/// cryptographic detail.
///
/// `created_at_unix_ms` is recorded by the producer at export time as
/// milliseconds since the Unix epoch. `device_fingerprint` is the stable,
/// human-comparable identity fingerprint of the device that produced the
/// bundle; it lets an operator confirm *which* device they are about to
/// adopt the identity of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigImportPreview {
    /// Application version string of the installation that produced the
    /// bundle. Opaque to this layer; format/parsing is the caller's concern.
    pub app_version: String,
    /// Storage layout the bundle was produced under.
    pub source_mode: ConfigSourceMode,
    /// Bundle creation time, milliseconds since the Unix epoch.
    pub created_at_unix_ms: i64,
    /// Profile the bundle's configuration belongs to.
    pub profile_id: ProfileId,
    /// Stable identity fingerprint of the producing device, for human
    /// confirmation. Adopting this bundle makes the target device present
    /// itself under this same identity.
    pub device_fingerprint: String,
}

/// Outcome of staging an import for later application.
///
/// Staging only validates and records the pending migration; it does not
/// apply it. Whether secret material the bundle carried can be re-derived
/// later, or whether unlock will be required after the migration is applied,
/// is reflected here so the caller can set operator expectations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedConfigImport {
    /// `true` when applying the staged migration will require the operator to
    /// re-enter their passphrase to unlock; `false` when the staged material
    /// is sufficient to unlock without further input.
    pub unlock_required_after_apply: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_mode_is_copy_and_comparable() {
        let a = ConfigSourceMode::Portable;
        let b = a;
        assert_eq!(a, b);
        assert_ne!(ConfigSourceMode::Portable, ConfigSourceMode::Installed);
    }

    #[test]
    fn preview_holds_descriptive_metadata() {
        let preview = ConfigImportPreview {
            app_version: "0.16.0".to_string(),
            source_mode: ConfigSourceMode::Portable,
            created_at_unix_ms: 1_700_000_000_000,
            profile_id: ProfileId::from("default".to_string()),
            device_fingerprint: "AB-CD-EF".to_string(),
        };
        assert_eq!(preview.source_mode, ConfigSourceMode::Portable);
        assert_eq!(preview.created_at_unix_ms, 1_700_000_000_000);
    }
}
