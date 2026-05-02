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

// Slice 2 Phase 3 · T10 — CLI `watch` decodes V3 envelope bytes from
// `InboundNotice.plaintext` to show human-readable text. Daemon uses the
// same helper internally via `ApplyInboundClipboardUseCase`.
pub use usecases::clipboard_sync::{decode_v3_bytes_to_snapshot, V3BlobRef};
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
