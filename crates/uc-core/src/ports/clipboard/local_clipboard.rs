//! Clipboard port - abstracts local clipboard access
//!
//! This port defines the interface for clipboard operations including
//! reading, writing, and monitoring clipboard changes.

use crate::clipboard::SystemClipboardSnapshot;
use anyhow::Result;
use async_trait::async_trait;

/// Clipboard port - abstracts local clipboard access
///
/// This trait provides a platform-agnostic interface to clipboard functionality,
/// allowing use cases to interact with the clipboard without depending on
/// platform-specific implementations.
#[async_trait]
pub trait SystemClipboardPort: Send + Sync {
    /// Read current clipboard content
    ///
    /// Returns the current clipboard content as a Payload, which can contain
    /// text, images, files, or other supported content types.
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot>;

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()>;
}
