//! Platform clipboard port for use cases
//!
//! This port defines the interface for use cases to read clipboard content.
//! It's a simplified version of SystemClipboardPort that only provides read access.
//!
//! ## Architecture Note
//!
//! This trait is implemented automatically for all types that implement
//! `SystemClipboardPort` through a blanket implementation. This allows
//! use cases to depend on this simpler interface while infrastructure
//! implementations can depend on the more feature-rich `SystemClipboardPort`.

use super::SystemClipboardPort;
use crate::clipboard::SystemClipboardSnapshot;
use anyhow::Result;
use async_trait::async_trait;

/// Platform clipboard port for use case layer.
///
/// This trait provides read-only access to clipboard for use cases.
/// It's automatically implemented for all `SystemClipboardPort` implementers.
#[async_trait]
pub trait PlatformClipboardPort: Send + Sync {
    /// Read current clipboard content
    ///
    /// Returns the current clipboard content as a snapshot.
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot>;
}

/// Blanket implementation: All SystemClipboardPort implementers automatically
/// implement PlatformClipboardPort.
///
/// This allows infrastructure implementations (which implement SystemClipboardPort)
/// to be used in use cases (which depend on PlatformClipboardPort).
#[async_trait]
impl<T> PlatformClipboardPort for T
where
    T: SystemClipboardPort,
{
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        SystemClipboardPort::read_snapshot(self)
    }
}
