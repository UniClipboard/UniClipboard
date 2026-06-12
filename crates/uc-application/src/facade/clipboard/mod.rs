//! `ClipboardSyncFacade` — Slice 2 Phase 2 public entry point.
//!
//! Per `uc-application/AGENTS.md` §11.4, the facade is the only type
//! external crates may hold. Internally it wraps
//! [`DispatchClipboardEntryUseCase`] and [`IngestInboundClipboardUseCase`]
//! and re-exports their public-shape types (`DispatchOutcome`,
//! `InboundClipboardNotice`, …) so CLI / daemon / Tauri never import
//! from `usecases::*` directly.

mod facade;

pub use facade::{
    ClipboardSyncDeps, ClipboardSyncError, ClipboardSyncFacade, DispatchEntryInput,
    DispatchEntryOutcome, DispatchEntryPerTarget, InboundAction, InboundNotice, IngestHandle,
};

// 投递状态视图相关类型——外部 crate 通过 `ClipboardSyncFacade::get_entry_delivery_view`
// 取得,渲染层使用 view 类型来绘制 UI;失败枚举沿用 `uc_core::clipboard::DeliveryFailureReason`,
// 外部按需直接从 uc-core 引入。
pub use crate::usecases::clipboard_sync::get_entry_delivery_view::{
    EntryDeliveryStatusView, EntryDeliveryTargetView, EntryDeliveryView, EntrySource,
    GetEntryDeliveryViewError,
};
