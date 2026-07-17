//! Port for choosing where a newly received file is written on disk.

use std::path::PathBuf;

use async_trait::async_trait;

/// Reserve a destination path for a newly received file.
///
/// Expresses the "where should this inbound file land" decision without
/// exposing how the destination is configured or laid out. Implementations
/// consult the user-configured save directory and reserve a unique path
/// inside it.
#[async_trait]
pub trait ReserveInboundFileTargetPort: Send + Sync {
    /// Reserve a destination path for an inbound file named `file_name`.
    ///
    /// When a user-configured save directory is set and currently writable,
    /// returns `Some(path)`: a collision-free absolute path inside that
    /// directory, atomically reserved so concurrent calls for the same name
    /// receive distinct paths. `file_name` is treated as a bare name; any
    /// path separators it contains are neutralized so the result never
    /// escapes the configured directory. A blank or otherwise unusable
    /// `file_name` is replaced with an implementation-chosen fallback name,
    /// so callers without a name may pass an empty string.
    ///
    /// Returns `None` when no save directory is configured, or the configured
    /// directory is currently unusable (missing, not writable). A `None`
    /// result signals the caller to fall back to its own managed storage
    /// layout; it is not an error and callers must not treat it as one.
    async fn reserve_target(&self, file_name: &str) -> Option<PathBuf>;
}

/// Resolve the directory the user configured to receive inbound content.
///
/// Answers "where does the user want this to land" without claiming a name or
/// creating anything, so a destination can be chosen while the final location
/// stays free of artifacts.
#[async_trait]
pub trait ResolveInboundSaveDirPort: Send + Sync {
    /// Return the configured save directory once it exists and is writable.
    ///
    /// The path is absolute. `None` means no directory is configured, or the
    /// configured one is currently unusable (missing and uncreatable, not
    /// writable); it is a fallback signal, not an error.
    ///
    /// Nothing inside the directory is created or reserved, so two calls with
    /// no intervening writes return the same path.
    async fn resolve_save_dir(&self) -> Option<PathBuf>;
}
