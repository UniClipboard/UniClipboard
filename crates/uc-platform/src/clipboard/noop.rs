//! Headless fallback for [`SystemClipboardPort`].
//!
//! Used when the native clipboard adapter cannot initialize — the usual
//! trigger on macOS is a non-bundled CLI launched from a shell without a
//! WindowServer connection, which makes `+[NSPasteboard generalPasteboard]`
//! return NULL and panic `objc2-app-kit`. Rather than taking down the
//! whole process, [`uc_bootstrap::assembly::create_platform_layer`] wraps
//! the real adapter in `catch_unwind` and substitutes this type on
//! failure. `read_snapshot` returns an empty snapshot, `write_snapshot`
//! silently no-ops — behaviour consistent with
//! `uc-platform/AGENTS.md` §9.3 ("明确返回 Unsupported").
//!
//! GUI / daemon paths never reach this fallback in practice; it exists
//! strictly for Slice 1 CLI commands (init/invite/join) that do not use
//! the clipboard at all.

use anyhow::Result;

use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopSystemClipboard;

impl SystemClipboardPort for NoopSystemClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        Ok(SystemClipboardSnapshot {
            ts_ms: 0,
            representations: Vec::new(),
            file_content_digests: Vec::new(),
        })
    }

    fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> Result<()> {
        Ok(())
    }
}
