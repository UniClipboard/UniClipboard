//! Slice 2 Phase 2 — clipboard sync use cases.
//!
//! * [`DispatchClipboardEntryUseCase`] — encrypt + fan-out a freshly
//!   captured clipboard entry to every reachable member.
//! * [`IngestInboundClipboardUseCase`] — subscribe to the receiver port,
//!   decrypt + re-broadcast each inbound payload as an application-level
//!   notice.
//!
//! Both are `pub(crate)` per `uc-application/AGENTS.md` §11.4. External
//! consumers (daemon / Tauri / CLI) reach them through
//! `ClipboardSyncFacade`.

pub(crate) mod active_state;
pub(crate) mod apply_inbound;
pub(crate) mod dispatch_entry;
pub(crate) mod get_entry_delivery_view;
pub(crate) mod ingest_inbound;
/// `pub` (not `pub(crate)`) because `decode_v3_bytes_to_snapshot` needs
/// a fully-public path for the CLI `watch` re-export at lib.rs root.
/// Individual private helpers inside stay scoped via their own
/// `pub(crate)` / no-modifier visibility.
pub mod payload_codec;
pub(crate) mod receive_gate;
pub(crate) mod resend_entry;
pub(crate) mod send_gate;
pub(crate) mod snapshot_from_entry;

pub(crate) use dispatch_entry::{
    DispatchClipboardEntryInput, DispatchClipboardEntryUseCase, DispatchOutcome, DispatchPerTarget,
    DispatchSyncError,
};
pub(crate) use ingest_inbound::{
    InboundAction, InboundClipboardNotice, IngestInboundClipboardUseCase, IngestSpawnHandle,
};
pub(crate) use payload_codec::encode_snapshot_to_v3_bytes;

// `ApplyInboundClipboardUseCase` is consumed by daemon (Phase 3 · T8)
// directly, so it gets re-exported at lib.rs root rather than staying
// behind `pub(crate)`.
pub use apply_inbound::{
    ApplyInboundClipboardUseCase, ApplyInboundError, ApplyInboundInput, ApplyOutcome,
    FileCacheBlobMaterializer, InboundBlobFetcher, InboundBlobMaterializer, InboundCapture,
    InboundWrite,
};

// Slice 2 Phase 3 · T10 — CLI `watch` decodes the V3 envelope payload
// so it can display human-readable text (daemon-sent payloads are now
// always enveloped). Exposed publicly because `InboundNotice.plaintext`
// is the facade-returned wire bytes, not representations.
pub use payload_codec::{
    decode_v3_bytes_to_snapshot, decode_v3_bytes_to_snapshot_and_blob_refs, V3BlobRef,
};
