mod background_blob_worker;
mod change_origin;
pub mod chunked_transfer;
mod durable_spool_queue;
mod normalizer;
mod payload_resolver;
mod representation_cache;
mod selection_resolver;
mod spool_janitor;
mod spool_manager;
mod spool_queue;
mod spool_scanner;
pub mod spooler_task;
mod staged_reconciler;
#[cfg(test)]
mod testing;
mod thumbnail_generator;

pub use background_blob_worker::BackgroundBlobWorker;

/// Builds a new `InMemoryClipboardChangeOrigin` and wraps it in an
/// `Arc<dyn ClipboardChangeOriginPort>`. Used by bootstrap to produce the shared
/// instance passed to [`init_clipboard_change_origin`]. Callers must not create
/// multiple independent instances — use [`init_clipboard_change_origin`] /
/// [`clipboard_change_origin`] to obtain the single shared singleton.
pub fn new_in_memory_change_origin(
) -> std::sync::Arc<dyn uc_core::ports::clipboard::ClipboardChangeOriginPort> {
    std::sync::Arc::new(change_origin::InMemoryClipboardChangeOrigin::new())
}
pub use chunked_transfer::{ChunkedDecoder, ChunkedEncoder, TransferCipherAdapter};
pub use durable_spool_queue::DurableSpoolQueue;
pub use normalizer::ClipboardRepresentationNormalizer;
pub use payload_resolver::ClipboardPayloadResolver;
pub use representation_cache::{CacheEntryStatus, RepresentationCache};
pub use selection_resolver::SelectionResolver;
pub use spool_janitor::SpoolJanitor;
pub use spool_manager::{SpoolEntry, SpoolManager};
pub use spool_queue::MpscSpoolQueue;
pub use spool_scanner::SpoolScanner;
pub use spooler_task::SpoolerTask;
pub use staged_reconciler::StagedReconciler;
pub use thumbnail_generator::InfraThumbnailGenerator;
pub use uc_core::ports::clipboard::SpoolRequest;

/// Module-level singleton for the shared `ClipboardChangeOriginPort` instance.
static CLIPBOARD_CHANGE_ORIGIN: std::sync::OnceLock<
    std::sync::Arc<dyn uc_core::ports::clipboard::ClipboardChangeOriginPort>,
> = std::sync::OnceLock::new();

/// Initialize the shared `ClipboardChangeOriginPort` singleton.
///
/// Idempotent: safe to call multiple times. If already initialized (e.g., by a test
/// helper), subsequent calls are no-ops. The first call wins.
pub fn init_clipboard_change_origin(
    shared: std::sync::Arc<dyn uc_core::ports::clipboard::ClipboardChangeOriginPort>,
) {
    // If already set (by a test helper or a previous call), do nothing.
    // Only initialize if not yet set.
    if CLIPBOARD_CHANGE_ORIGIN.get().is_none() {
        let _ = CLIPBOARD_CHANGE_ORIGIN.set(shared);
    }
}

/// Return a clone of the shared `ClipboardChangeOriginPort` singleton.
///
/// Returns `None` if [`init_clipboard_change_origin`] has not been called yet.
pub fn clipboard_change_origin(
) -> Option<std::sync::Arc<dyn uc_core::ports::clipboard::ClipboardChangeOriginPort>> {
    CLIPBOARD_CHANGE_ORIGIN.get().cloned()
}
