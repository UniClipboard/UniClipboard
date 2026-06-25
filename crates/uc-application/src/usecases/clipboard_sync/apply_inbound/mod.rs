//! `ApplyInboundClipboardUseCase` — daemon-side inbound clipboard
//! processing pipeline (Slice 2 Phase 3 · T4).
//!
//! ## Flow
//!
//! 1. **Dedup short-circuit**: if `snapshot_hash` already exists in the
//!    local `clipboard_event` table, return `DuplicateSkipped`. Skips
//!    persist + OS-clipboard write — Phase 3 acceptance #4 guarantees a
//!    repeat copy from a peer doesn't double-write the user's clipboard.
//! 2. **Envelope decode**: V3 → `SystemClipboardSnapshot`. Decode failure
//!    is non-fatal (`DecodeFailed` outcome) — corrupted payloads from a
//!    misbehaving peer don't crash the daemon's ingest loop.
//! 3. **Capture pipeline**: reuse `CaptureClipboardUseCase` with origin
//!    `RemotePush` so the entry, event, normalised representations,
//!    cache, spool, and (optional) search index all match the local
//!    capture path's schema (D5 decision).
//! 4. **OS clipboard write**: via `ClipboardWriteCoordinator` with
//!    `RemotePush` intent — arms a self-write echo record (a content hash
//!    guard plus a one-shot next-origin override, under the coordinator's echo
//!    budget) so the daemon's own clipboard watcher doesn't re-dispatch the
//!    just-written content (write-back loop defence; distinct from the inbound
//!    idempotency dedup in [`timing`]).
//!    The **full** snapshot (every V3-decoded representation) is handed
//!    to the coordinator; the platform layer internally decides whether
//!    to atomically write multiple formats (Windows today) or to narrow
//!    to the paste-priority rep via `SelectRepresentationPolicyV1`
//!    (macOS / Linux fallback today).
//!
//! Step ordering (3 → 4) matters: capture commits the event before the
//! OS write fires, so when the watcher consumes the origin guard it
//! already sees the persisted row.
//!
//! ## 模块拆分
//!
//! 这个模块按职责再细分到三个子文件,顶层 `mod.rs` 只持有公共类型
//! (`ApplyInboundInput` / `ApplyOutcome` / `ApplyInboundError`) 与 re-export:
//!
//! * [`ports`] — `InboundCapture` / `InboundWrite` 抽象 + 它们对
//!   `CaptureClipboardUseCase` / `ClipboardWriteCoordinator` 的 blanket impl,
//!   保持 use case 在测试里可 mock。
//! * [`materializer`] — `InboundBlobMaterializer` / `InboundBlobFetcher`
//!   抽象 + `FileCacheBlobMaterializer` 默认实现,负责把入站 blob refs 拉到
//!   本机缓存 / 写回 representation。
//! * [`usecase`] — `ApplyInboundClipboardUseCase` 主流程,只负责编排:
//!   dedup → V3 解码 → emit IncomingPending → materialize → capture → OS write。
//!
//! ## Testability
//!
//! `CaptureClipboardUseCase` and `ClipboardWriteCoordinator` are
//! concrete structs with 7+2 port dependencies. Holding them as
//! `Arc<dyn Trait>` via two thin internal abstractions
//! ([`InboundCapture`] / [`InboundWrite`]) keeps the use case mockable
//! without requiring tests to construct full real implementations.
//! Production wires the concrete types via the blanket impls in
//! [`ports`].

use bytes::Bytes;
use thiserror::Error;
use uc_core::ids::{DeviceId, EntryId};
use uc_observability::FlowId;

mod materializer;
mod ports;
mod timing;
mod usecase;

#[cfg(test)]
mod tests;

pub use materializer::{FileCacheBlobMaterializer, InboundBlobFetcher, InboundBlobMaterializer};
pub use ports::{InboundCapture, InboundWrite};
pub use usecase::ApplyInboundClipboardUseCase;

/// Caller-supplied input mapped from the facade's public `InboundNotice`.
///
/// Keeping this struct separate from `crate::facade::clipboard::InboundNotice`
/// avoids the use case importing from the facade layer (§11.4 keeps the
/// arrow `facade → use case`, never the reverse).
#[derive(Debug, Clone)]
pub struct ApplyInboundInput {
    pub from_device: DeviceId,
    pub snapshot_hash: String,
    pub plaintext: Bytes,
    pub flow_id: Option<FlowId>,
}

/// Result of one `execute` call. Daemon's worker maps each variant to a
/// distinct telemetry path (WS event for `Applied`, debug log for
/// `DuplicateSkipped`, warn log for `DecodeFailed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// New content — persisted + OS clipboard written. WS event fires.
    Applied { entry_id: EntryId },
    /// `snapshot_hash` was already present in the local DB. No persist,
    /// no OS write, no WS event.
    DuplicateSkipped {
        snapshot_hash: String,
        existing_entry_id: EntryId,
    },
    /// V3 envelope was malformed. Frame dropped silently except for a
    /// warning log; receiver loop keeps running.
    DecodeFailed { reason: String },
}

#[derive(Debug, Error)]
pub enum ApplyInboundError {
    #[error("dedup query failed: {0}")]
    DedupQuery(String),
    #[error("capture pipeline failed: {0}")]
    Capture(String),
    #[error("clipboard write failed: {0}")]
    WriteCoordinator(String),
    #[error("internal: {0}")]
    Internal(String),
}
