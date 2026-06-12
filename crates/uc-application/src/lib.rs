//! Application-layer workflows for UniClipboard.

pub mod clipboard_capture;
pub mod clipboard_write;
pub mod deps;
pub mod facade;
pub mod file_sync;
pub mod sync_planner;

// D16-2: deps re-exports so composition roots (uc-bootstrap, uc-tauri,
// uc-daemon) can depend on `uc_application::*` directly and the legacy
// `uc_app::*` shims can be retired.
pub use deps::{
    AppDeps, ClipboardPorts, DevicePorts, SearchPorts, SecurityPorts, StoragePorts, SystemPorts,
};

// Slice 2 Phase 3 · T4 — public use case consumed directly by daemon
// `InboundClipboardSyncWorker` (T8). Lives inside `usecases::clipboard_sync`
// (which is `pub(crate)`) so Phase 2 internals stay encapsulated; we
// re-export only the small public surface here.
pub use usecases::clipboard_sync::{
    ApplyInboundClipboardUseCase, ApplyInboundError, ApplyInboundInput, ApplyOutcome,
    FileCacheBlobMaterializer, InboundBlobFetcher, InboundBlobMaterializer, InboundCapture,
    InboundWrite,
};

// Note: V3 envelope codec helpers (decode_v3_bytes_to_snapshot,
// decode_v3_bytes_to_snapshot_and_blob_refs, V3BlobRef) used to live
// here. Per AGENTS.md §11.4.3 they now route through `facade/` —
// import them as `uc_application::facade::decode_v3_bytes_to_snapshot`
// etc. The implementations stay in `usecases::clipboard_sync` but the
// crate boundary only exposes them via the facade.
pub mod file_transfer;
pub mod membership;
pub(crate) mod pairing_inbound;
pub(crate) mod pairing_invitation;
pub(crate) mod pairing_outbound;
pub mod proof;
pub mod trusted_peer;
/// `pub` (not `pub(crate)`) only because Slice 2 Phase 3 · T10 needs a
/// publicly-reachable path to `usecases::clipboard_sync::payload_codec
/// ::decode_v3_bytes_to_snapshot` for the CLI `watch` command. Every
/// sub-module inside `usecases` keeps its own `pub(crate)` visibility
/// cap, so only explicitly `pub` items inside leak out; the public
/// surface stays minimal.
pub mod usecases;
