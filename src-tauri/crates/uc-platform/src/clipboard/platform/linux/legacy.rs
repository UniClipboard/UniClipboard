//! Legacy Linux clipboard backend on top of `clipboard_rs`.
//!
//! This is the X11-only path that has been in production since before native
//! Wayland support was added. It is currently used for both X11 sessions and
//! (as a degraded fallback) Wayland sessions whose compositor does not advertise
//! any data-control protocol. Phase 3 will replace this with a native `x11rb`
//! implementation; Phase 4 then drops the `clipboard_rs` dependency on Linux.

use anyhow::Result;
use async_trait::async_trait;
use clipboard_rs::ClipboardContext;
use std::sync::{Arc, Mutex};
use tracing::{debug, debug_span, error};
use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;

use crate::clipboard::common::CommonClipboardImpl;

pub struct LegacyLinuxClipboard {
    inner: Arc<Mutex<ClipboardContext>>,
}

impl LegacyLinuxClipboard {
    pub fn new() -> Result<Self> {
        let context = ClipboardContext::new()
            .map_err(|e| anyhow::anyhow!("Failed to create clipboard context: {}", e))?;
        Ok(Self {
            inner: Arc::new(Mutex::new(context)),
        })
    }
}

#[async_trait]
impl SystemClipboardPort for LegacyLinuxClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        let span = debug_span!("platform.linux.legacy.read_clipboard");
        span.in_scope(|| {
            let mut ctx = match self.inner.lock() {
                Ok(ctx) => ctx,
                Err(poison) => {
                    error!("Failed to lock clipboard context (poisoned mutex)");
                    return Err(anyhow::anyhow!(
                        "Clipboard mutex poisoned: {}",
                        poison.to_string()
                    ));
                }
            };
            let snapshot = CommonClipboardImpl::read_snapshot(&mut ctx)?;

            debug!(
                formats = snapshot.representations.len(),
                total_size_bytes = snapshot.total_size_bytes(),
                "Captured system clipboard snapshot"
            );

            Ok(snapshot)
        })
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        let span = debug_span!(
            "platform.linux.legacy.write_clipboard",
            representations = snapshot.representations.len(),
        );
        span.in_scope(|| {
            let mut ctx = self.inner.lock().map_err(|poison| {
                error!("Failed to lock clipboard context in write_snapshot (poisoned mutex)");
                anyhow::anyhow!(
                    "mutex poisoned locking inner in write_snapshot: {}",
                    poison.to_string()
                )
            })?;
            CommonClipboardImpl::write_snapshot(&mut ctx, snapshot)?;

            debug!("Wrote clipboard snapshot to system");
            Ok(())
        })
    }
}
