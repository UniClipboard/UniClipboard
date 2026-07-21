mod background_blob_worker;
mod broadcasting_advance;
mod change_origin;
pub mod chunked_transfer;
mod durable_spool_queue;
mod normalizer;
mod payload_resolver;
mod representation_cache;
mod selection_resolver;
mod spool_janitor;
mod spool_manager;
mod spool_scanner;
mod staged_reconciler;
#[cfg(test)]
mod testing;
mod thumbnail_generator;

pub use background_blob_worker::BackgroundBlobWorker;
pub use broadcasting_advance::BroadcastingAdvance;

/// Builds an engine-owned `InMemorySelfWriteLedger`.
///
/// Each engine instance must own a separate ledger so self-write attribution
/// cannot survive shutdown and affect a later engine in the same process.
pub fn new_in_memory_change_origin(
) -> std::sync::Arc<dyn uc_core::ports::clipboard::SelfWriteLedgerPort> {
    std::sync::Arc::new(change_origin::InMemorySelfWriteLedger::new())
}
pub use chunked_transfer::{ChunkedDecoder, ChunkedEncoder, TransferCipherAdapter};
pub use durable_spool_queue::DurableSpoolQueue;
pub use normalizer::ClipboardRepresentationNormalizer;
pub use payload_resolver::ClipboardPayloadResolver;
pub use representation_cache::{CacheEntryStatus, RepresentationCache};
pub use selection_resolver::SelectionResolver;
pub use spool_janitor::SpoolJanitor;
pub use spool_manager::{SpoolEntry, SpoolManager};
pub use spool_scanner::SpoolScanner;
pub use staged_reconciler::StagedReconciler;
pub use thumbnail_generator::InfraThumbnailGenerator;
pub use uc_core::ports::clipboard::SpoolRequest;
