//! Ports expressing whole-installation configuration migration intents.
//!
//! Three intents are kept separate because they have different
//! preconditions, different directions, and different consumers:
//!
//! * exporting an installation's configuration into a portable, password
//!   protected bundle;
//! * inspecting a bundle's descriptive metadata before committing to it;
//! * staging a bundle so a subsequent restart can adopt it.
//!
//! None of these signatures expose how a bundle is packaged or protected. A
//! password, where one is taken, is an opaque operator secret; the bundle is
//! referenced only by a filesystem location.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::crypto::domain::Passphrase;

use super::error::ConfigMigrationError;
use super::model::{ConfigImportPreview, StagedConfigImport};

/// Export the current installation's configuration into a self-protected
/// bundle written to a destination path.
///
/// Requires an unlocked, initialized installation; the unlocked session is the
/// authorization gate for releasing the device identity and encrypted data.
#[async_trait]
pub trait ExportConfigBundlePort: Send + Sync {
    /// Produce a configuration bundle and write it to `destination`.
    ///
    /// The bundle is protected by the installation's own key material, so no
    /// separate export secret is taken; reading it back later requires the
    /// space passphrase. On success returns the path the bundle was written to
    /// (the producer may normalize or finalize `destination`).
    ///
    /// Returns:
    /// * [`ConfigMigrationError::Locked`] when the session is not unlocked;
    /// * [`ConfigMigrationError::NotInitialized`] when there is nothing to
    ///   export;
    /// * [`ConfigMigrationError::Io`] on write failure.
    async fn export_bundle(&self, destination: &Path) -> Result<PathBuf, ConfigMigrationError>;
}

/// Read a bundle's non-secret descriptive metadata without applying it.
///
/// Read-only: produces no side effects and changes no state. Intended to let
/// an operator confirm what they are about to adopt before any commitment.
#[async_trait]
pub trait PreviewConfigImportPort: Send + Sync {
    /// Decode the descriptive metadata of the bundle at `source`, unlocking it
    /// with `password`.
    ///
    /// Returns:
    /// * [`ConfigMigrationError::InvalidPasswordOrCorrupt`] when `password` is
    ///   wrong or the bundle is corrupt;
    /// * [`ConfigMigrationError::IncompatibleBundle`] when the bundle is too
    ///   new or otherwise unsupported;
    /// * [`ConfigMigrationError::Io`] when the source cannot be read.
    async fn preview_import(
        &self,
        password: &Passphrase,
        source: &Path,
    ) -> Result<ConfigImportPreview, ConfigMigrationError>;
}

/// Stage a bundle so a later restart applies it as this installation's
/// configuration.
///
/// Staging validates the bundle and records the pending migration; it does not
/// touch the live configuration immediately. Applying on the next restart
/// replaces whatever configuration the target currently holds, if any.
#[async_trait]
pub trait StageConfigImportPort: Send + Sync {
    /// Validate the bundle at `source` using `password` and record it as a
    /// pending migration to be applied on the next restart. Applying replaces
    /// the target's existing configuration, if any.
    ///
    /// Returns:
    /// * [`ConfigMigrationError::InvalidPasswordOrCorrupt`] when `password` is
    ///   wrong or the bundle is corrupt;
    /// * [`ConfigMigrationError::IncompatibleBundle`] when the bundle is too
    ///   new or otherwise unsupported;
    /// * [`ConfigMigrationError::Io`] when the source cannot be read or the
    ///   pending migration cannot be recorded.
    async fn stage_import(
        &self,
        password: &Passphrase,
        source: &Path,
    ) -> Result<StagedConfigImport, ConfigMigrationError>;
}
