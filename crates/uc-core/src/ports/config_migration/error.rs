//! Error semantics for configuration migration ports.
//!
//! Variants describe *domain-level* failure reasons an operator or caller can
//! act on. They never name a concrete cipher, key-derivation function, archive
//! format, or storage backend; such detail belongs to the adapter's logs.

use thiserror::Error;

/// Failure reasons for exporting or staging a configuration bundle.
#[derive(Debug, Error)]
pub enum ConfigMigrationError {
    /// The current session is locked, so the operation is not authorized.
    ///
    /// Exporting requires an unlocked session as the authorization gate that
    /// proves the operator holds the passphrase; a locked session is refused
    /// before any material is read.
    #[error("session is locked")]
    Locked,

    /// The source installation has nothing to export because it was never
    /// initialized.
    #[error("source installation is not initialized")]
    NotInitialized,

    /// The supplied password was wrong, or the bundle is corrupt.
    ///
    /// These two cases are intentionally indistinguishable to avoid revealing
    /// whether a password guess was correct.
    #[error("invalid password or corrupt bundle")]
    InvalidPasswordOrCorrupt,

    /// The bundle is structurally valid but cannot be accepted by this build —
    /// for example it was produced by a newer, unsupported version.
    ///
    /// `reason` is a stable, non-secret explanation suitable for surfacing to
    /// the operator.
    #[error("incompatible bundle: {reason}")]
    IncompatibleBundle {
        /// Non-secret, operator-facing explanation of the incompatibility.
        reason: String,
    },

    /// An input/output failure occurred while reading the source, writing the
    /// destination, or recording the staged migration.
    ///
    /// `details` must be free of secret material and path contents.
    #[error("io failure: {details}")]
    Io {
        /// Non-secret description of the I/O failure.
        details: String,
    },

    /// Any other internal failure not covered by the variants above.
    ///
    /// `details` must be free of secret material.
    #[error("internal error: {details}")]
    Internal {
        /// Non-secret description of the internal failure.
        details: String,
    },
}
