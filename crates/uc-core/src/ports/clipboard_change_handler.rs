//! Clipboard change handler port
//!
//! This port defines the callback interface for handling clipboard change events
//! from the platform layer. It follows the Dependency Inversion Principle:
//! - Platform layer (low-level) depends on this abstraction
//! - App layer (high-level) implements this interface

use crate::SystemClipboardSnapshot;
use anyhow::Result;

/// Callback handler for clipboard change events.
///
/// The platform layer calls this when clipboard content changes.
/// The snapshot is already read by the platform layer.
#[async_trait::async_trait]
pub trait ClipboardChangeHandler: Send + Sync {
    /// Called when clipboard content changes.
    ///
    /// # Parameters
    /// - `snapshot`: The current clipboard state captured by platform layer
    async fn on_clipboard_changed(&self, snapshot: SystemClipboardSnapshot) -> Result<()>;
}
